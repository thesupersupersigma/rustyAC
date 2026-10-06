# Electronics and driver aids

Source: `acs.exe` + `acs.pdb`, read through the local index only (`re/decomp/` pseudo-C, `re/types/`
struct layouts, `tools/re_query.py`, `tools/disasm.py` for exact constants and operation order,
`tools/ini_survey.py` over the 113 cars in `cardata/`, and the commented Kunos example car in the SDK).
Raw material: `re/car/electronics/` (90 plain copies of the pseudo-C cited here). Scratch helpers:
`re/scratch/electronics/`. Nothing in the game folder, the Ghidra project or git was changed.
No Rust was written.

Wheel order: `tyres[0]` = left front, `[1]` = right front, `[2]` = left rear, `[3]` = right rear.
Step numbers such as "#16" are positions in `Car::stepComponents` 0x1402764d0 (same numbering as
`docs/map/car_step.md`): #1 `BrakeSystem::step`, #2 `EDL::step`, #3 suspensions, #4 `Tyre::step` x4,
#8 `Kers::step`, #9 `ERS::step`, #10 `SteeringSystem::step`, #11 `AutoBlip::step`,
#12 `AutoShifter::step`, #13 `GearChanger::step`, #14 `Drivetrain::step` (the engine runs inside it),
#15 `AntirollBar::step` x2, #16 `ABS::step`, #17 `TractionControl::step`, #18 `SpeedLimiter::step`,
#27 `StabilityControl::step`.

---

## 1. Plain-English summary

The car has a handful of small helpers that watch the wheels and overrule the driver for a moment.
Traction control looks at how much the driven wheels are spinning faster than the road; when the
worst one is over a limit it cuts the engine completely, and it only re-checks a fixed number of
times per second, so the cut comes in short pulses.
Anti-lock brakes do the mirror image: when a wheel is turning much slower than the road under
braking, the brake on that wheel (or on that axle, or on all four wheels, depending on the car) is
released completely until the next check.
The electronic differential lock compares the two driven wheels and gently brakes the one that
spins faster, which pushes drive to the other wheel.
The pit-lane limiter cuts the engine above 80 km/h and brakes above 81 km/h whenever any wheel is
on a pit-lane surface.
"Stability control" from the game options is not a real ESP: it is a helping hand that twists the
whole car body back when the rear tyres slide more than the fronts. Computer-driven cars always get
a different version of that twist, based on how sideways the body is travelling.
Auto-blip, auto-shifter and auto-clutch do not touch the car at all: they edit the driver's pedals
and paddles just before the gearbox and engine read them.
All of these are on/off or simple ramps; none of them has a model of a real ECU.
Finally there is one generic "lookup controller" (`DynamicController`) that other systems use to
turn a car signal (speed, gear, slip, steering ...) into a setting through tables, for example an
active differential, brake bias, turbo boost or hybrid power map.
`escmode.ini` has nothing to do with any of this: it only positions a camera (by its name, the one
shown behind the Esc / pause menu).

---

## 2. Classes

### Physics (in `ksPhysicsAC.lib`)

| Class | Size | What it is for |
|---|---|---|
| `TractionControl` | 0xa8 | `Car::tractionControl` (Car+0xa00). Engine cut on drive-wheel spin. Members: `isPresent`, `isActive`, `slipRatioLimit`, `isInAction`, `frequency` (seconds between checks), `minSpeedMS`, `timeAccumulator`, `currentMode`, `lastValue` (never used in the code read), `valueCurve` (levels). |
| `ABS` | 0xa8 | `Car::abs` (Car+0x958). Writes `Tyre::absOverride`. Members: `isPresent`, `isActive`, `slipRatioLimit`, `frequency`, `channels` (1, 2 or 4), `timeAccumulator`, `valueCurve`, `currentMode`, `currentValue`. |
| `EDL` | 0x38 | `Car::edl` (Car+0x3428). Electronic differential lock: brakes the faster driven wheel. Members: gains, dead zones and torques for power and coast, `outLevel`, `outBrakeTorque`, `speedDiff`, `leftTyreIndex`, `rightTyreIndex`. |
| `StabilityControl` | 0x18 | `Car::stabilityControl` (Car+0x34f0). Yaw-assist torque on the car body. Members: `gain`, `useBeta`, `car`, `maxGain`. |
| `SpeedLimiter` | 0x10 | `Car::speedLimiter` (Car+0xaa8). Pit-lane limiter. Members: `shoudLimit` (sic), `isLimiting`, `car`. |
| `DynamicController` | 0x28 | Generic lookup controller: `car`, `stages` (vector of `DynamicControllerStage`), `ready`. Shared by brakes, drivetrain, turbos, ERS, KERS, 4-wheel steering, anti-roll bars. |
| `DynamicControllerStage` | 0xa0 | One stage: `inputVar`, `combinatorMode`, `lut` (`Curve`), `filter`, `upLimit`, `downLimit`, `currentValue`, `constValue`. |
| `TurboDynamicController` | 0x38 | `turbo` pointer + a `DynamicController` + `isWastegate`. Lives in `Engine::turboControllers`. |
| `DrivetrainControllers` | 0x20 | Four optional controllers: `awdFrontShare`, `awdCenterLock`, `singleDiffLock`, `awd2`. |
| `AutoBlip` | 0xa8 | `Car::autoBlip` (Car+0x3340). Throttle blip on downshift: `isActive`, `blipProfile`, `blipStartTime`, `isElectronic`, `blipPerformTime`. |
| `AutoShifter` | 0x28 | `Car::autoShift` (Car+0x33e8). Automatic gearbox assist: `isActive`, `changeUpRpm`, `changeDnRpm`, `slipThreshold`, `gasCutoff`, `gasCutoffTime`. |
| `Autoclutch` | 0x1a8 | `Car::autoClutch` (Car+0x3090). Automatic clutch at standstill and during shifts: `rpmMin`, `rpmMax`, `clutchSpeed`, `useAutoOnStart`, `useAutoOnChange`, `isForced`, `clutchSequence`, the two shift profiles, `clutchValueSignal`. |
| `GearChanger` | 0x18 | `Car::gearChanger` (Car+0x3410). Turns paddle edges into gear requests. |
| `Engine::electronicOverride` | float | Engine+0x1f8 (Car+0x5e8). 0..1 factor on the throttle. Written by TC and the pit limiter, consumed and reset to 1 by `Engine::step`. |
| `BrakeSystem::electronicOverride` | float | Minimum brake pedal. Written by the pit limiter, consumed and reset to 0 by `BrakeSystem::step`. |
| `Tyre::absOverride` | float | Per-wheel 0..1 factor on the brake torque. Written by ABS (and by the AI). |
| `CarControls` | 0x34 | Driver inputs. Holds the cockpit buttons `absUp/absDn`, `tcUp/tcDn`, `turboUp/turboDn`, `engineBrakeUp/Dn`, `MGUKDeliveryUp/Dn`, `MGUKRecoveryUp/Dn`, `MGUHMode`. |

### Game side (not physics; listed because they change the physics settings)

| Class | What it is |
|---|---|
| `DrivingAssistManager` (0x68) | Reads `cfg/assists.ini` once at session start and applies the options to the player car. |
| `CarAvatar` setters / cyclers | `setAbsEnabled`, `setTcEnabled`, `setStabilityControl`, `setAutoBlip`, `setAutoShifter`, `setAutoClutchEnabled`, `cycleAbsMode`, `cycleTcMode`, `cycleEngineBrake`, `cycleERSPower`, `cycleERSRecovery`, `cycleERSHeatCharging`, `setTurboBoost`. Most of them queue a small job for the physics thread. |
| `AbsNotifier`, `TractionControlNotifier`, `EngineBrakeNotifier`, `MGUKDeliveryNotifier`, `MGUKRecoveryNotifier`, `MGUHModeNotifier`, `TurboBoostNotifier`, `BrakeBiasNotifier` | Watch the cockpit-button bits of the player car and call the cyclers; also show the on-screen message. |
| `SetupElectronics` | "ELECTRONICS" tab of the setup screen: spinners that call the same cyclers. |
| `ElectronicAidsForm`, `ACEssentials`, `SystemMessage` | Display only. |
| `ACPlugin` | Plugin interface with direct setters (`setABS`, `setTC`, `setAutoShift`, `setStabilityControl`). |
| `ServerDrivingAssists` (0xc) | Online: `tc`, `abs` (int 0/1/2), `stability`, `autoClutch` (bool) sent by the server; they overrule the local options. |

---

## 3. Key functions

### Loading / init

```
Car::Car                                         0x14026bf00
├─ (inline constructors) ABS: channels = 4, currentValue = 1, everything else 0/false
│                        TractionControl, EDL, StabilityControl: all 0/false
├─ BrakeSystem::init                             0x14028d690   electronicOverride = 0; ctrl_ebb.ini; calls BrakeSystem::loadINI (steer_brake_controller.ini)
├─ ERS::init                                     0x140291c30   ctrl_ers_<n>.ini / ctrl_ers_front_<n>.ini
├─ Kers::init                                    0x1402b7360   kers.ini, key CONTROLLER (a controller file name)
├─ SteeringSystem::init                          0x1402b80d0   ctrl_4ws.ini
├─ Drivetrain::init                              0x140266dc0   (calls in this order)
│  ├─ Engine::init                               0x140285e10   electronicOverride = 1
│  │  └─ Engine::loadINI                         0x140286100   [COAST_SETTINGS], throttle.lut, ctrl_turbo<n>.ini, ctrl_wastegate<n>.ini
│  ├─ Drivetrain::loadINI                        0x140267440   ctrl_awd2.ini, [GEARBOX] AUTO_CUTOFF_TIME
│  └─ Drivetrain::initControllers                0x140267070   ctrl_single_lock.ini, ctrl_awd_front_share.ini, ctrl_awd_center_lock.ini
├─ Autoclutch::init                              0x1402b87d0   → Autoclutch::loadINI 0x1402b8890; subscribes to drivetrain.evOnGearRequest
├─ AutoBlip::init                                0x1402b9910   → AutoBlip::loadINI 0x1402b99b0; subscribes to drivetrain.evOnGearRequest
├─ AutoShifter::init                             0x1402ba020   → AutoShifter::loadINI 0x1402ba050
├─ GearChanger::init                             0x1402bab40
├─ EDL::init                                     0x1402babf0   electronics.ini [EDL]
├─ Car::buildARBS                                0x14026f750   ctrl_arb_front.ini, ctrl_arb_rear.ini
├─ ABS::init                                     0x14028ec50   electronics.ini [ABS] or [ABS_V2]
├─ TractionControl::init                         0x14028fa40   electronics.ini [TRACTION_CONTROL]
├─ SpeedLimiter::init                            0x1402bb900   only stores the car pointer (address shared with LapInvalidator::init)
└─ StabilityControl::init                        0x1402bfa30   gain = 0, useBeta = false, maxGain = 30

DynamicController::DynamicController(car, path)  0x1402af330   the file loader (section 4.6)
├─ DynamicControllerStage::DynamicControllerStage 0x1402b0780
├─ INIReader::getCurve                           0x140235040   LUT= inline "(|x=y|...)" or a file name
└─ lagToLerpDeltaK                               0x14005d7c0   FILTER → per-step blend factor
DynamicController::DynamicController()           0x1402b0760   empty controller (ready = false)

Session start (game thread)
Sim::Sim                                         0x140192070   also reads escmode.ini (pause-menu camera)
└─ DrivingAssistManager::DrivingAssistManager    0x1400fbd90   cfg/assists.ini → CarAvatar setters (player car only)
AIDriver::AIDriver                               0x140298c60   switches the car's own ABS/TC/auto-shifter off, auto-clutch and auto-blip on
```

### Every physics step

```
PhysicsEngine::step                              0x140264760
├─ evOnPreStep → PhysicsAvatar::stepCommandQueue 0x1401223e0   runs queued jobs: ABS::cycleMode 0x14028eae0,
│                                                              TractionControl::cycleMode 0x14028f8e0, Engine::setCoastSettings
│                                                              0x140288010, ERS::setPowerController 0x140292fc0, flag writes
└─ Car::step                                     0x140275da0
   ├─ Car::pollControls                          0x140274e70   provider → Car::controls (AI: AIDriver::acquireControls 0x14029baa0
   │                                                           sets stabilityControl.useBeta/gain and the AI's own absOverride);
   │                                                           wheel rumble uses ABS::isInAction 0x14028f5d0
   ├─ Autoclutch::step                           0x1402b9590   writes controls.clutch (→ Autoclutch::stepSequence 0x1402b97a0)
   └─ Car::stepComponents                        0x1402764d0
      ├─ #1  BrakeSystem::step                   0x14028e640   pedal = max(electronicOverride, controls.brake); then override = 0
      ├─ #2  EDL::step                           0x1402bb460   adds brake torque to the faster driven wheel
      ├─ #4  Tyre::step x4                       0x140283800   brake = max(brakeTorque * absOverride, handBrakeTorque)
      ├─ #11 AutoBlip::step                      0x1402b9ef0   raises controls.gas during a downshift blip
      ├─ #12 AutoShifter::step                   0x1402ba7f0   sets controls.gearUp / gearDn, zeroes gas after an upshift
      ├─ #13 GearChanger::step                   0x1402bab50   → Drivetrain::gearUp 0x1402669f0 / gearDown
      ├─ #14 Drivetrain::step                    0x14026b130
      │      ├─ Drivetrain::stepControllers      0x14026b200   DynamicController::eval 0x1402b0c00 → DynamicController::getInput 0x1402b0d70
      │      └─ step2WD 0x1402694e0 / step4WD / step4WD_new 0x14026ad80
      │         └─ Engine::step                  0x1402880e0   gas *= electronicOverride; ...; electronicOverride = 1
      │            └─ Engine::stepTurbos         0x140288900   turbo / wastegate controllers
      ├─ #16 ABS::step                           0x14028f610   writes tyres[i].absOverride
      ├─ #17 TractionControl::step               0x140290200   writes engine.electronicOverride = 0
      ├─ #18 SpeedLimiter::step                  0x1402bb910   engine.electronicOverride = 0, brakeSystem.electronicOverride
      │      └─ RaceEngineer::getOptimalBrake    0x14027c320
      └─ #27 StabilityControl::step              0x1402bfa50   body->addLocalTorque
```

Not in the step: `TractionControl::getCurrentMode` 0x14028f9b0, `ABS::getCurrentMode` 0x14028ebc0
(display), `DynamicController::isReady` 0x1402bb630 (returns `ready`; only the telemetry form asks).
Two static helpers do run in the step: `DynamicController::getRearSpeedRatio` 0x1402b1230 is called by
`getInput` for `REAR_SPEED_RATIO`, and both it and `DynamicController::getOversteerFactor` 0x1402b11d0
are called by `Telemetry::step` 0x1402bf5b0 (#21, player car only); `getInput` has its own inlined
copy of the oversteer formula.
Small game-side leftovers: `DrivingAssistManager::update` 0x1400fcb20 re-writes the ideal-line visible
flag every frame; `Autoclutch::getDownshiftSequenceDuration` 0x14029d3a0 and
`Autoclutch::setDownshiftProfile` 0x14029f840 are used only by `AIDriver::AIDriver` (section 6).

---

## 4. Data it reads

A missing key gives 0 (`INIReader::getFloat` / `getInt` return 0 for an absent key). electronics.ini is opened through `Car::getConfigPath("content/cars/<car>/data/electronics.ini")`; the `CURVE` level files and the controller files `ctrl_ebb.ini`, `steer_brake_controller.ini`, `ctrl_4ws.ini`, `ctrl_single_lock.ini`, `ctrl_awd*.ini`, `ctrl_turbo<n>.ini`, `ctrl_wastegate<n>.ini` and the kers.ini `CONTROLLER` file are opened with the plain `content/cars/<car>/data/<file>` path, so a `data_<configName>` folder never overrides them (`ctrl_arb_*.ini` and `ctrl_ers_<n>.ini` do go through `getConfigPath`).

### 4.1 electronics.ini (113 of 113 cars)

Only four section names are read by the game: `[TRACTION_CONTROL]`, `[ABS]`, `[ABS_V2]`, `[EDL]`.
There is no `[SPEED_LIMITER]`, no `[AUTOBLIP]` and no `DISPLAY_SLIP_RATIO_LIMIT` in `acs.exe` (no such
string literal; none of the 113 cars has them either).

`[TRACTION_CONTROL]` (113 cars) - `TractionControl::init`. Before the file is read: `slipRatioLimit = 0.2`,
`isPresent = true`, `isActive = true`, `frequency = 0.05`, `currentMode = 0`. If the file cannot be
opened these values stay.

| Key | Member | Notes |
|---|---|---|
| `SLIP_RATIO_LIMIT` | `slipRatioLimit` | slip ratio (0.10 = 10 %). Typical 0.08 - 0.12. |
| `MIN_SPEED_KMH` | `minSpeedMS` | converted with `Speed::fromKMH` 0x140239970 = `value * 0.2777778` (float 0x3e8e38e4, a multiply, not a division by 3.6). Typical 25 - 50. |
| `CURVE` | `valueCurve` | file name in the car's data folder, loaded with `Curve::load` 0x140206a50 only when not empty. 58 cars name a file (`traction_control.lut` 56, `t_c.lut` 2), 55 leave it blank. After loading, `currentMode` = first index `i` with `valueCurve.getValue(i) == slipRatioLimit` (exact float compare); if none matches it stays 0. |
| `PRESENT` | `isPresent` | 71 cars = 1, 42 = 0. |
| `ACTIVE` | `isActive` | starts switched on. 85 cars = 1. |
| `RATE_HZ` | `frequency = 1 / RATE_HZ` | only when not 0, else `frequency` stays 0.05 s. Typical 100 - 200 (ks_ferrari_f2004: 80). |

`[ABS]` (101 cars) or `[ABS_V2]` (12 cars) - `ABS::init`. If `[ABS_V2]` exists it is used (version 2),
otherwise `[ABS]`. Defaults before reading: `slipRatioLimit = 0.2`, `isPresent = true`,
`isActive = true`, `frequency = 0.05`, `channels = 4` (from the constructor inside `Car::Car`).

| Key | Member | Notes |
|---|---|---|
| `SLIP_RATIO_LIMIT` | `slipRatioLimit` | positive number; the test uses `-slipRatioLimit`. Typical 0.10 - 0.13. |
| `PRESENT` | `isPresent` | |
| `ACTIVE` | `isActive` | |
| `CURVE` | `valueCurve` | levels file (`abs_control.lut` 6 cars, `abs_settings.lut` 2 cars). Quirk confirmed in the disassembly: the "is it empty" test reads the key from the chosen section, but the file name itself is always read from `[ABS]`. All 12 `[ABS_V2]` cars leave `CURVE` blank, so it never matters in `cardata/`. `currentMode` is found the same way as for TC. |
| `RATE_HZ` | `frequency = 1 / RATE_HZ` | only when not 0. Typical 100 - 333. |
| `CHANNELS` | `channels` | `[ABS_V2]` only. Must be 1, 2 or 4; anything else (also a missing key = 0) prints "CRITICAL ERROR ... only 1, 2 and 4 supported" and calls `ksGenerateCrash`. 11 cars use 1, one uses 4. With `[ABS]` the value stays 4. |

`[EDL]` (46 cars; 33 with `PRESENT=1`) - `EDL::init`. Only read when the section exists. Defaults:
`isPresent = isActive = false`, gains 0.01, dead zones 0, torques 2000.

| Key | Member | Notes |
|---|---|---|
| `PRESENT`, `ACTIVE` | `isPresent`, `isActive` | both must be true for EDL to work. |
| `BRAKE_TORQUE_POWER`, `BRAKE_TORQUE_COAST` | `brakeTorquePower`, `brakeTorqueCoast` | Nm at full level. |
| `DEAD_ZONE_POWER`, `DEAD_ZONE_COAST` | `deadZonePower`, `deadZoneCoast` | relative speed difference that is ignored. |
| `MAX_SPIN_POWER`, `MAX_SPIN_COAST` | `wheelSpeedGainPower = 1 / (MAX_SPIN_POWER - deadZonePower)`, same for coast | relative speed difference at which the full torque is reached. If the result is not finite (the two values are equal) the game prints "CRITICAL ERROR", calls `ksGenerateCrash`, and the next line sets the gain to 0. |

Which wheels: `leftTyreIndex = 2`, `rightTyreIndex = 3`; if `drivetrain.tractionType` is `FWD` (1) or
`AWD` (2) they become 0 and 1. (`RWD` = 0 and `AWD_NEW` = 3 keep the rear wheels.)

### 4.2 Level tables (`traction_control.lut`, `abs_control.lut` ...)

Plain `Curve` files, one `index|slip_ratio_limit` pair per line, e.g. ks_ferrari_f2004
`traction_control.lut`: `0|0.04 1|0.05 2|0.06 3|0.08 ... 9|0.16`. The number of lines is the number of
levels; `currentMode` is the 0-based line. The value is fetched with `Curve::getValue((float)index)`
0x140206990, which is a linear interpolation (`(v[i]-v[i-1]) * (x-r[i-1]) / (r[i]-r[i-1]) + v[i-1]`), so
the limit in use is the interpolated float, not the parsed one. In all 353 levels of the files in
`cardata/` both are bit-identical (checked by simulating the float32 formula in
`re/scratch/electronics/`), and 61 of 62 aid/car pairs find their starting level; ferrari_458_s3 TC
does not (limit 0.10 is not in its table), so it starts on level index 0 with limit 0.10.
(Re-run by the reviewer with `re/scratch/verify_electronics/levels.py`: same 62 / 61 / 353 / 0.)

`Curve::getValue` edge cases (read in the pseudo-C): `x <= references[0]` returns the first value,
`x` above the last reference returns the last value, an empty curve prints "EMPTY CURVE" and returns 0.
`Curve::load` clears the curve first, then reads the file line by line; a line counts only if splitting
it on `|` gives exactly two parts (`reference|value`), anything else is skipped without an error. If the
plain file cannot be opened nothing is read, so the curve stays empty: `getCount() == 0`, the aid has
no levels and `cycleMode` is a plain on/off (section 5.11). When a `<data folder>.acd` archive exists
the lines come from `Curve::loadEncrypted` instead (not read here). `Curve::addValue` 0x140205ae0 is a
plain push_back on the two vectors: no sorting and no de-duplication, so the table is used in file order.

### 4.3 Game options: `cfg/assists.ini` in the user's Documents folder, `[ASSISTS]`

Read once by `DrivingAssistManager::DrivingAssistManager` and applied to car 0 (the player car).

| Key | Effect |
|---|---|
| `IDEAL_LINE` | graphics only (`track->idealLine` visible flag). |
| `AUTO_BLIP` | `CarAvatar::setAutoBlip` → `autoBlip.isActive`; when switched off on a car with `drivetrain.isShifterSupported`, also `drivetrain.autoCutOffTime = 0`. |
| `STABILITY_CONTROL` | percent. `stabilityControl.gain = max(0, value * 0.01)`. Forced to 0 online when the server forbids it. |
| `AUTO_SHIFTER` | `autoShift.isActive`, and tells the controls provider (provider virtual +0x38, `DICarControl::onAutoShifterChanged`). |
| `ABS` | 0 = off: `abs.isActive = false` and `abs.isPresent = false`. 1 = factory: `abs.isActive = true`, `isPresent` as in the car file. 2 = on: both true (forces ABS onto cars that do not have it). Online the server value is used. |
| `TRACTION_CONTROL` | same three values for `tractionControl.isActive` / `isPresent`. |
| `AUTO_CLUTCH` | `autoClutch.useAutoOnStart`; if on, also `useAutoOnChange = true`. Forced on when `cfg/controls.ini [HEADER] INPUT_METHOD` is `X360` or `KEYBOARD`; online it is forced off when the server's `autoClutch` flag is false (this wins over the input method); a request to switch it off is ignored when the provider says `isAutoclutchNeeded`. |
| `DAMAGE`, `FUEL_RATE`, `TYRE_WEAR`, `TYRE_BLANKETS`, `SLIPSTREAM` | not electronics (only offline): `PhysicsEngine::mechanicalDamageRate = value * 0.01`, `fuelConsumptionRate`, `tyreConsumptionRate`, `allowTyreBlankets`, slipstream gains. |

`ABS` and `TRACTION_CONTROL` are integers 0 / 1 / 2. All three values write `isActive`, so for the
player car the `ACTIVE` key of electronics.ini is always overwritten at session start; it only
survives on cars that this manager does not touch (and `AIDriver::AIDriver` switches those off for AI
cars). Note for "factory": the first `ABS::step` check does `isActive = isActive && isPresent`, so a
car with `PRESENT=0` ends up inactive. For TC, `isPresent == false` makes `step` clear `isActive` on
every call.

The other readers of `cfg/assists.ini` are display only: `CarHintLoader::CarHintLoader` 0x140068860
(loading-screen hints), `IdealLine::IdealLine` 0x140109d10 and
`SetupElectronics::checkDrivingAssist` 0x14016cb40 (setup screen). `Car::steerAssist` (Car+0x3c70,
car.ini `[CONTROLS] STEER_ASSIST`) is not a driving aid despite its name: it is the exponent of the
force-feedback curve (steering map).

### 4.4 Driver-edit aids: drivetrain.ini

| Section / key | Member | Notes |
|---|---|---|
| `[AUTOBLIP] LEVEL, POINT_0, POINT_1, POINT_2` | `AutoBlip::blipProfile` = points `(0, 0) (POINT_0, LEVEL) (POINT_1, LEVEL) (POINT_2, 0)`; `blipPerformTime` = largest reference (`POINT_2`) | times in ms. 113 cars. |
| `[AUTOBLIP] ELECTRONIC` | `isElectronic` | 57 cars = 1: the blip works even when the auto-blip assist is off. (`AutoBlip::init` sets `isActive = true` and `blipStartTime = 0` before loading; the options / the AI then overwrite `isActive`.) |
| `[AUTO_SHIFTER] UP, DOWN, SLIP_THRESHOLD, GAS_CUTOFF_TIME` | `changeUpRpm`, `changeDnRpm`, `slipThreshold`, `gasCutoffTime` | 108 cars. If the section is missing the same four keys are read from `ai.ini [GEARS]`. Defaults: 0, 4000, 0.8, 0.5. `UP = 0` means "work it out from the engine" (section 5.7). |
| `[AUTOCLUTCH] MIN_RPM, MAX_RPM` | `rpmMin`, `rpmMax` | both replaced by 1500 / 2500 if either is 0. |
| `[AUTOCLUTCH] USE_ON_CHANGES` | `useAutoOnChange` | 88 cars = 1. |
| `[AUTOCLUTCH] FORCED_ON` | `isForced` | only read when `[HEADER] VERSION > 2`. 34 cars = 1. |
| `[AUTOCLUTCH] UPSHIFT_PROFILE, DOWNSHIFT_PROFILE` | names of sections holding `POINT_0..2` in ms → `upshiftProfile` / `downshiftProfile` = `(0, 1) (P0 * 0.001, 0) (P1 * 0.001, 0) (P2 * 0.001, 1)` (float multiply by 0.001, not a division) | clutch value over time in seconds. The profile is built for every name that is not exactly `NONE` - also for an empty / missing key, which then gives four points at time 0. Upshift: `NONE` in 92 cars; downshift: `NONE` in 1 car. Confirmed line by line in `Autoclutch::loadINI`. |
| `[GEARBOX] AUTO_CUTOFF_TIME` | `Drivetrain::autoCutOffTime = value * 0.001` | throttle cut after an upshift (drivetrain map). |

### 4.5 Engine-brake setting: engine.ini `[COAST_SETTINGS]` (2 of 113 cars: ks_ferrari_sf15t, ks_ferrari_sf70h)

| Key | Member | Notes |
|---|---|---|
| `LUT` | `Engine::gasCoastOffsetCurve` | inline table, e.g. `(0=0.205|1=0.20|...|12=0.0)`: index → throttle offset. |
| `DEFAULT` | `coastSettingsDefaultIndex`, then `Engine::setCoastSettings(DEFAULT)` | 9 in both cars. |
| `ACTIVATION_RPM` | `coastEntryRpm = ACTIVATION_RPM + data.minimum` | 4500. Note the idle rpm is added here and subtracted again nowhere (section 5.8). |

### 4.6 DynamicController files (`ctrl_*.ini`, `steer_brake_controller.ini`)

Sections `[CONTROLLER_0]`, `[CONTROLLER_1]`, ... are read in order until the first missing number.
Each becomes one stage.

| Key | Member | Notes |
|---|---|---|
| `INPUT` | `inputVar` | one of the names in section 5.9. Unknown name: error message, and a fatal error when `INIReader::crashAtError` is set. |
| `COMBINATOR` | `combinatorMode` | `ADD` = 1, `MULT` = 2, anything else = 0 (error message only). |
| `LUT` | `lut` | through `INIReader::getCurve`: if the value contains `(` and `)` it is an inline list `(|x=y|x=y|)` (the text between the brackets is split on `|`, each item on `=`; an item that does not give exactly two parts is a "CRITICAL ERROR ... is not a valid curve" + `ksGenerateCrash`), otherwise a file name next to the ini (loaded with `Curve::load` if it exists). Not read for `INPUT=CONST`. An empty table is fatal only with `crashAtError`. |
| `CONST_VALUE` | `constValue` | only for `INPUT=CONST`. |
| `FILTER` | `filter = lagToLerpDeltaK(FILTER, 0.004, 0.003)` = `(1/0.003 * 0.004) * (1 - FILTER) * (1/0.003)` ≈ `444.44 * (1 - FILTER)` | 0 = no smoothing, 0.99 = slow. |
| `UP_LIMIT`, `DOWN_LIMIT` | `upLimit`, `downLimit` | clamp of the running result after this stage; ignored when both are 0. |

Who loads which file (each only if the file exists; counts over `cardata/`):

| File | Loader | Output goes to | Cars |
|---|---|---|---|
| `ctrl_ebb.ini` | `BrakeSystem::init` | brake bias | 10 |
| `steer_brake_controller.ini` | `BrakeSystem::loadINI` 0x14028d870 | extra torque on one rear wheel | 3 |
| `ctrl_single_lock.ini` | `Drivetrain::initControllers` 0x140267070, only when `tractionType == RWD` | `diffPreLoad` (power and coast ramps forced to 0) | 12 ship it, 10 load it (the `AWD2` cars ks_porsche_cayenne and ks_porsche_macan never open theirs) |
| `ctrl_awd_front_share.ini` | `Drivetrain::initControllers`, only when `tractionType == AWD` | `awdFrontShare` | 0 |
| `ctrl_awd_center_lock.ini` | `Drivetrain::initControllers`, only when `tractionType == AWD` | `awdCenterDiff.preload` | 1 |
| `ctrl_awd2.ini` | `Drivetrain::loadINI` | AWD2 lock (`Drivetrain::step4WD_new`) | 8 |
| `ctrl_turbo<n>.ini`, `ctrl_wastegate<n>.ini` | `Engine::loadINI` | turbo `maxBoost` / `wastegate` | 6 / 0 |
| `ctrl_ers_<n>.ini`, `ctrl_ers_front_<n>.ini` | `ERS::init` | ERS power maps (selectable from the cockpit) | 5 / 0 |
| `[KERS] CONTROLLER` in kers.ini | `Kers::init` | KERS input | 1 (ferrari_laferrari `controller_kers.ini`; 2 cars have a kers.ini, the other leaves the key blank) |
| `ctrl_4ws.ini` | `SteeringSystem::init` | rear-wheel steering | 2 |
| `ctrl_arb_front.ini`, `ctrl_arb_rear.ini` | `Car::buildARBS` | anti-roll bar stiffness | 2 |

Other `ctrl_*.ini` names found in `cardata/` (`ctrl_aero_louvers`, `ctrl_bargeboard`,
`ctrl_brake_pressure`, `ctrl_diff_coast`, `ctrl_diff_power`, `ctrl_wheel` ..., one car each) are not
referenced by any string in `acs.exe`. They all belong to one mod car
(`vrc_formula_alpha_2026_csp`), whose setup.ini names them - a Custom Shaders Patch feature that the
original executable never reads. The wing controllers in aero.ini are a different mechanism
(aero map).

### 4.7 escmode.ini (113 of 113 cars) - not electronics

Read only by `Sim::Sim` (confirmed: the only function that uses the string): `[SETTINGS] POSITION`
(3 floats, default `0, 1.4, 3.2`) and `FOV` go to `cameraManager->cameraOnBoardFree`
(`CameraOnBoardFree`). No physics code reads it. That this is the view behind the Esc / pause menu is
my interpretation from the file name; how `CameraOnBoardFree` is selected was not read (unverified).

---

## 5. Each step: inputs, maths, outputs

All speeds are m/s unless a `* 3.6` is shown. `Car::getSpeed()` 0x140272160 returns
`valueCache.speed`, the body speed cached by `Car::stepPreCacheValues` 0x1402768c0 before `Car::step`.
`dt` is 0.003.

### 5.1 `TractionControl::step(dt)` (#17) - confirmed against the disassembly

```
if !isPresent: isInAction = false; isActive = false; return
v = |Car::getVelocity()|                      # 0x140272940 = body->getVelocity(); sqrt(x²+y²+z²), 0 if the sum is 0
if v < minSpeedMS: isInAction = false; timeAccumulator = 0; return
timeAccumulator = dt + timeAccumulator
if timeAccumulator < frequency:
    if isInAction: engine.electronicOverride = 0          # hold the cut between checks
    return
timeAccumulator = 0
if isActive and Car::getSpeed() > 1.0:
    s = 0
    for i in 0..3: if tyres[i].driven and tyres[i].status.slipRatio > s: s = tyres[i].status.slipRatio
    if s > slipRatioLimit: engine.electronicOverride = 0
isInAction = (engine.electronicOverride != 1.0)
```

- Measured: the largest positive `status.slipRatio` of the driven tyres (`Tyre::driven`).
- Action: all or nothing. `Engine::step` multiplies the throttle by `electronicOverride` (0) - there is
  no partial cut, no ignition retard, no torque target.
- Rate: the check happens when the float32 sum of `dt` reaches `frequency` (`sum >= frequency`), then
  the sum restarts at 0. With `dt = 0.003` the real period is a whole number of steps: `RATE_HZ` 80 →
  5 steps (15 ms), 100 → 4 steps, 120 → 3 steps, 200 / 250 / 333 → 2 steps. For every `RATE_HZ` value
  that occurs in `cardata/` (TC and ABS; float32 emulation in `re/scratch/verify_electronics/levels.py`,
  not a game run): 15 → 23 steps, 35 → 10, 60 → 6, 70 and 80 → 5, 100 → 4, 120 - 165 → 3,
  170 - 333 → 2, and 400 / 500 / 667 (TC of 12 cars, e.g. ferrari_458_s3 with 667) → **every step**,
  because `1 / RATE_HZ` is then below one `dt`. The 0.05 s default (missing or zero `RATE_HZ`) → 17 steps.
  The period is constant (the sum restarts from exactly 0 each time).
- A cut decided at a check lasts until the next check (the "hold" branch re-writes 0 every step
  because the engine resets the override every step).
- The level only changes `slipRatioLimit`.

### 5.2 `ABS::step(dt)` (#16)

```
timeAccumulator = dt + timeAccumulator
if timeAccumulator < frequency: return                  # absOverride keeps its value
timeAccumulator = 0
isActive = isActive && isPresent
if isActive and Car::getSpeed() * 3.6 > 20.0:
    L = -slipRatioLimit;  locked(i) = tyres[i].status.slipRatio < L
    channels == 1: f = (locked(0) or locked(1)) ? 0 : 1;  r = (locked(2) or locked(3)) ? 0 : 1
                   all four absOverride = min(f, r)
    channels == 2: absOverride[0] = absOverride[1] = f;  absOverride[2] = absOverride[3] = r
    channels == 4: absOverride[i] = locked(i) ? 0 : 1
    (any other value: nothing is written)
else:
    absOverride[0..3] = 1;  currentValue = 1
```

- Measured: `status.slipRatio` of each tyre (negative under braking).
- Action: `Tyre::absOverride` is 0 or 1. The tyre uses `max(brakeTorque * absOverride, handBrakeTorque)`,
  so 0 releases the foot brake completely on that wheel; the handbrake is not affected.
- Same timer rule as TC. `currentValue` is only ever written as 1.
- `ABS::isInAction()` = any of the four `absOverride != 1.0` (used for wheel rumble and display).

### 5.3 `EDL::step(dt)` (#2)

```
if !(isActive and isPresent): return
wl = |tyres[leftTyreIndex].status.angularVelocity|;  wr = |tyres[rightTyreIndex].status.angularVelocity|
if wl == 0 or wr == 0: return
speedDiff = max(wl, wr) / min(wl, wr) - 1
if car.accG.z <= 0: T = brakeTorqueCoast;  dz = deadZoneCoast;  g = wheelSpeedGainCoast
else:               T = brakeTorquePower;  dz = deadZonePower;  g = wheelSpeedGainPower
if wr < wl:  x = (wl / wr - 1) - dz;  fast = leftTyreIndex
else:        x = (wr / wl - 1) - dz;  fast = rightTyreIndex
outLevel = clamp(g * max(x, 0), 0, 1)
tyres[fast].inputs.brakeTorque = outLevel * T + tyres[fast].inputs.brakeTorque
outBrakeTorque = T * outLevel
```

"Power" or "coast" is chosen from the sign of the longitudinal acceleration `accG.z`, not from the
throttle. The torque is added on top of what `BrakeSystem::step` wrote a moment earlier, and it is
also multiplied by `absOverride` in the tyre. No timer: it runs every step.

### 5.4 `StabilityControl::step(dt)` (#27) - confirmed against the disassembly

```
if gain <= 0: return
if useBeta:                                             # AI cars only (section 6)
    v = body->getLocalVelocity()                        # IRigidBody +0xb8
    if v.z <= 5.0: return
    beta = clamp(atanf(v.x / v.z), -0.52358997, 0.52358997)       # ±30°
    torque = body->getMass() * ((beta * gain) * 10.0)             # IRigidBody +0x28
    body->addLocalTorque((0, torque, 0))                # IRigidBody +0xe0
else:                                                   # player assist
    front = |tyres[1].slipAngleRAD + tyres[0].slipAngleRAD|
    rear  = |tyres[3].slipAngleRAD + tyres[2].slipAngleRAD|
    d = (rear - front) * 0.5
    if d > 0 and Car::getSpeed() * 3.6 > 5.0:
        sign = tyres[2].slipAngleRAD > 0 ? 1 : (tyres[2].slipAngleRAD < 0 ? -1 : 0)
        torque = -((((body->getMass() * d) * gain) * maxGain) * sign)      # maxGain = 30
        body->addLocalTorque((0, torque, 0))
```

It is a yaw torque about the body's own up axis, pushed straight into the rigid body. It does not
brake wheels and does not cut the engine. `getMass()` is the mass of the main body only (not the
wheels or the fuel tank). With the option at 100 % (`gain = 1`) the player torque is
`30 * mass * d` Nm with `d` in radians; for an AI car (`gain = 2`) it is `20 * mass * beta`.

### 5.5 `SpeedLimiter::step(dt)` (#18)

```
shoudLimit = any tyres[i].surfaceDef exists and has isPitlane;  isLimiting = false
if shoudLimit:
    kmh = Car::getSpeed() * 3.6
    if kmh > 80.0: engine.electronicOverride = 0;  isLimiting = true
    if kmh > 81.0: brakeSystem.electronicOverride = RaceEngineer::getOptimalBrake()
```

`getOptimalBrake` = the smaller of the front and rear "pedal that just reaches tyre grip":
`front = (D1*load1 + D0*load0) * 0.5 * loadedRadius[0] / (brakePower * frontBias)`,
`rear = (D3*load3 + D2*load2) * 0.5 * loadedRadius[2] / (brakePower * (1 - frontBias))`, with
`Di = Tyre::getCorrectedD(Tyre::getDX(load_i))`. 80 and 81 km/h are literals; there is no ini key and no
driver button in this class. It runs for every car, player and AI.

### 5.6 `AutoBlip::step(dt)` (#11)

```
if car.isControlsLocked or Car::getSpeed() * 3.6 < 5.0: return
if !(isElectronic or isActive): return
t = ksPhysics->physicsTime - blipStartTime             # ms, double
if 0 <= t < blipPerformTime and blipProfile.getCount() == 4:
    controls.gas = clamp(max(controls.gas, blipProfile.getValue(t)), 0, 1)
```

`blipStartTime` is set by the gear-request handler (lambda 0x1402b9880): on a change-down request, if
`controls.clutch > 0.1`, `blipStartTime = physicsTime`.

### 5.7 `AutoShifter::step(dt)` (#12), `GearChanger::step` (#13)

```
if !PhysicsEngine::hasSessionStarted(300.0) or !isActive or drivetrain.currentGear == 0: return
if changeUpRpm == 0:                                    # first use only
    lim = Engine::getLimiterRPM()                       # engine virtual +0x08, 0x140285a50 = (int)((float)data.limiter * limiterMultiplier)
    changeUpRpm = (int)((float)min(lim, (int)Engine::getMaxPowerRPM()) * 0.98)     # integer min, then float multiply
    changeDnRpm = (int)(Engine::getMaxTorqueRPM() * 1.1)
if controls.gearUp or controls.gearDn: return           # the driver's own paddle wins
slipping = RaceEngineer::getDrivingTyresSlip() > slipThreshold and Car::getSpeed() > 5.0
if !Drivetrain::isChangingGear() and (controls.clutch > 0.99 or currentGear == 1) and !slipping:
    rpm = Drivetrain::getEngineRPM()
    if rpm > changeUpRpm and currentGear < gearCount - 1 and controls.gas > 0.2 and gasCutoff <= 0:
        controls.gearUp = true;  gasCutoff = gasCutoffTime
    dn = (currentGear == 3) ? (int)(changeDnRpm * 0.65) : changeDnRpm
    if rpm < dn and currentGear > 2 and controls.clutch > 0.85 and gasCutoff <= 0: controls.gearDn = true
if Car::getSpeed() < 2.0 and !isChangingGear() and controls.gas < 0.1 and gasCutoff <= 0 and currentGear > 2:
    controls.gearDn = true
if gasCutoff > 0: gasCutoff -= dt;  controls.gas = 0
```

`getDrivingTyresSlip` 0x14027bc20 = larger `status.ndSlip` of the front pair for FWD, of the rear pair
otherwise. Gear index: 0 = reverse, 1 = neutral, 2 = first (my reading, consistent with the `GEAR`
controller input being `currentGear - 1`).

`GearChanger::step`: if `controls.requestedGearIndex != -1` (H-shifter) →
`Drivetrain::setCurrentGear(index, false)`; otherwise a rising edge of `controls.gearUp` /
`gearDn` calls `Drivetrain::gearUp` / `gearDown`. A successful `gearUp` sets
`drivetrain.cutOff = autoCutOffTime` (if not 0) and puts the box in neutral (`currentGear = 1`) until
the request completes; while `cutOff > 0` the drivetrain feeds the engine a throttle of 0 and counts
`cutOff -= dt`. Confirmed in `Drivetrain::step2WD` and, with the same lines, in `Drivetrain::step4WD`
0x14026a220; `step4WD_new` has no copy of its own because it starts by calling `step2WD`.

### 5.8 Throttle chain inside `Engine::step` (only the electronics part)

In this order: `Engine::getThrottleResponseGas` 0x140285b90 (throttle.lut) → push-to-pass → engine-brake
setting → rev limiter → `gas = electronicOverride * gas`; `gasUsage = gas` → torque maths → near the
end `electronicOverride = 1.0`.

Engine-brake setting (only when `gasCoastOffset > 0`):

```
f = clamp((rpm - data.minimum) / (float)coastEntryRpm, 0, 1)
o = gasCoastOffset * f
gas = clamp((1 - o) * gas + o, 0, 1)
```

So the "engine brake" level is a floor under the throttle that fades in with rpm; less engine braking =
more throttle offset. `Engine::setCoastSettings(i)`: if `0 <= i <= gasCoastOffsetCurve.getCount()`,
`gasCoastOffset = gasCoastOffsetCurve.getValue((float)i)`. Because TC and the pit limiter multiply
after this, their cut also removes the offset.

### 5.9 `DynamicController::eval()` and its inputs

```
out = 0
for each stage in file order:
    v = (inputVar == CONST) ? constValue : lut.getValue(getInput(inputVar))
    prev = currentValue
    if |v - prev| >= 0.001:
        k = clamp(filter * 0.003, 0, 1)                 # 0.003 is a literal, not the dt argument
        v = (v - prev) * k + prev
    currentValue = v
    combinatorMode 0: out = 0;   1 (ADD): out = out + currentValue;   2 (MULT): out = out * v
    if not (downLimit == 0 and upLimit == 0):
        if out > upLimit: out = upLimit   elif out < downLimit: out = downLimit
return out
```

`k = clamp(1.3333 * (1 - FILTER), 0, 1)`, so any `FILTER <= 0.25` follows the table instantly. The
smoothing state (`currentValue`) lives in the stage and advances once per `eval` call. The first
stage should be `ADD` (the running value starts at 0).

`DynamicControllerInput` (numbers confirmed from the constructor's disassembly) and what
`getInput` returns:

| # | `INPUT=` | Value |
|---|---|---|
| 0 | (undefined) | 0 |
| 1 | `BRAKE` | `controls.brake` |
| 2 | `GAS` | `controls.gas` |
| 3 | `LATG` | `car.accG.x` |
| 4 | `LONG` | `car.accG.z` |
| 5 | `STEER` | `controls.steer` (-1..1) |
| 6 | `SPEED_KMH` | `Car::getSpeed() * 3.6` |
| 7 | `GEAR` | `(float)(drivetrain.currentGear - 1)` |
| 8 | `SLIPRATIO_MAX` | larger `status.slipRatio` of the driven pair (RWD rear, FWD front); AWD (type 2): the larger value of whichever pair has the larger maximum, i.e. the maximum of all four; other types 0 |
| 9 | `SLIPRATIO_AVG` | RWD `(sr[3] + sr[2]) * 0.5`; FWD `(sr[1] + sr[0]) * 0.5`; AWD `(sr[1] + sr[0] + sr[2] + sr[3]) * 0.25`; other types 0 |
| 10 | `SLIPANGLE_FRONT_AVG` | `(sa[1] + sa[0]) * 57.29578 * 0.5` (signed) |
| 11 | `SLIPANGLE_REAR_AVG` | `(sa[3] + sa[2]) * 57.29578 * 0.5` (signed) |
| 12 | `SLIPANGLE_FRONT_MAX` | `max(|sa[0]|, |sa[1]|) * 57.29578` |
| 13 | `SLIPANGLE_REAR_MAX` | `max(|sa[2]|, |sa[3]|) * 57.29578` |
| 14 | `OVERSTEER_FACTOR` | `((|sa[3]| + |sa[2]|) * 0.5 - (|sa[0]| + |sa[1]|) * 0.5) * 57.29578` |
| 15 | `REAR_SPEED_RATIO` | `((w[3] + w[2]) * 0.5) / ((w[1] + w[0]) * 0.5)`, 0 if the front average is 0 (`w` = `status.angularVelocity`) |
| 16 | `STEER_DEG` | `car.steerLock * controls.steer` |
| 17 | `CONST` | `constValue` (no table) |
| 18 | `RPMS` | `Drivetrain::getEngineRPM()` |
| 19 | `WHEEL_STEER_DEG` | `car.finalSteerAngleSignal` = `(steerLock * controls.steer) / steerRatio` (0 if not finite), written only in `Car::step` just before `Autoclutch::step` |
| 20 | `LOAD_SPREAD_LF` | `load[0] / (load[0] + load[1])` |
| 21 | `LOAD_SPREAD_RF` | `load[1] / (load[1] + load[0])` |
| 22 | `AVG_TRAVEL_REAR` | `(travel(susp[3]) + travel(susp[2])) * 0.5 * 1000` (mm; `ISuspension` virtual +0x48 = `getStatus()`, `travel` is the first float of the returned `SuspensionStatus`) |
| 23 | `SUS_TRAVEL_LR` | `travel(susp[2]) * 1000` |
| 24 | `SUS_TRAVEL_RR` | `travel(susp[3]) * 1000` |

`sa` = `status.slipAngleRAD`, `sr` = `status.slipRatio`. `AWD_NEW` (type 3) cars get 0 from
`SLIPRATIO_MAX` / `SLIPRATIO_AVG`.

Consumers (what each does with the result):

| Caller | Use |
|---|---|
| `BrakeSystem::step` #1 | `ebbController` → bias; `steerBrake.controller` → rear wheel torque (brakes map) |
| `Kers::step` #8 0x1402b7e10, `ERS::step` #9 0x1402930e0 | power request; ERS clamps `controller` and `controllerFront` to 0..1 |
| `SteeringSystem::step` #10 0x1402b81b0 | rear steer angle (steering map) |
| `Drivetrain::stepControllers` #14 | `awdFrontShare = eval()`; `awdCenterDiff.preload = (eval() - 20) * clamp((kmh - 5) * 0.05, 0, 1) + 20` with power and coast set to 0; `diffPreLoad = eval()` with `diffPowerRamp = diffCoastRamp = 0` |
| `Drivetrain::step4WD_new` #14 | AWD2 lock torque (drivetrain map) |
| `Engine::stepTurbos` #14 | per controller: `turbo->data.maxBoost = eval()` or, if `isWastegate`, `turbo->data.wastegate = eval()` |
| `AntirollBar::step` #15 0x1402bb640 | bar stiffness (suspension map) |

### 5.10 `Autoclutch::step(dt)` (in `Car::step`, before `stepComponents`)

```
if !clutchSequence.isDone:
    if Car::getSpeed() * 3.6 > 5: stepSequence(dt); return     # clutch = profile(currentTime), time += dt
    clutchSequence.isDone = true
if !useAutoOnStart and !isForced: return
rpm = (float)(drivetrain.engine.velocity * 0.15915507152579872 * 60.0);  target = 1    # double maths (mulsd), one rounding to float
gear 0 or 2 (reverse, first):
    if rpmMin <= rpm <= rpmMax: target = (rpm - rpmMin) / (rpmMax - rpmMin);  clutchValueSignal = target
    if rpm < rpmMin: target = 0;  clutchValueSignal = 0
gear 1 (neutral):
    if kmh < 5: if controls.gas > 0.2: clutchValueSignal = 1  else: target = 0; clutchValueSignal = 0
other gears:
    if rpm < rpmMin: target = 0;  clutchValueSignal = 0
move clutchValueSignal toward target by at most dt * clutchSpeed (clutchSpeed = 1.0)
controls.clutch = clamp(clutchValueSignal, 0, 1)
```

`Autoclutch::onGearRequest` 0x1402b9350: when `useAutoOnChange` and `clutchValueSignal > 0.01`, a
change-down / change-up request starts the down / up profile (only if that profile has exactly 4
points). The clutch itself belongs to the drivetrain map.

### 5.11 Levels: `cycleMode(dir)` (same code in `TractionControl` and `ABS`)

```
if !isPresent: return
n = valueCurve.getCount()
if n == 0: isActive = !isActive; return                 # single level: plain on/off
if !isActive: isActive = true;  mode = (dir < 1) ? n - 1 : 0
elif dir > 0: mode = (currentMode + 1) % n;  currentMode = mode;  if mode == 0: isActive = false; return
else:         if currentMode == 0: isActive = false; return
              mode = currentMode - 1
currentMode = mode
slipRatioLimit = valueCurve.getValue((float)currentMode)
```

So the order going up is level 1 ... level n, then off. When it wraps to off, `slipRatioLimit` keeps
the last value. `getCurrentMode()` returns `(0, 0)` when inactive, `(currentMode + 1, n)` with a table,
`(1, 1)` without.

---

## 6. Connections to other systems

### Who writes this system's inputs

| Input | Written by | When |
|---|---|---|
| `tyres[i].status.slipRatio`, `slipAngleRAD`, `ndSlip`, `load`, `surfaceDef` | `Tyre::step` #4 | same step, before ABS / TC / pit limiter / stability control; **previous** step for EDL (#2) and for controllers evaluated in #1 |
| `tyres[i].status.angularVelocity` | `Tyre::step` (free wheels), `Drivetrain::step` #14 (driven wheels) | EDL at #2 sees the previous step's value |
| `tyres[i].driven` | drivetrain set-up | constant |
| `car.accG` | `Car::step`, before `stepComponents` | from the body velocity change over the last rigid-body step |
| `valueCache.speed` | `Car::stepPreCacheValues` | start of the step |
| `controls.*` | controls provider in `Car::pollControls`, then `Car::step` overrides, `Autoclutch::step`, `AutoBlip::step`, `AutoShifter::step` | same step |
| `isActive` / `isPresent` / `gain` / levels | game thread through `PhysicsAvatar::executeOnPhysicsThread` 0x140121f20; jobs run in `evOnPreStep` before the cars | asynchronous to the physics step |

### Who reads this system's outputs (one-step delays)

| Output | Writer (#) | Reader (#) | Delay |
|---|---|---|---|
| `tyres[i].inputs.brakeTorque` (EDL part) | EDL #2 | `Tyre::step` #4, `Kers` #8, `ERS` #9, `Drivetrain` #14 | none (same step) |
| `tyres[i].absOverride` | ABS #16 (AI: `AIDriver::stepSuperhuman` 0x1402a2a60 in `pollControls`) | `Tyre::step` #4, `Kers` #8, `ERS` #9, `Drivetrain::step2WD` #14 | **next step** for ABS |
| `engine.electronicOverride` | TC #17, pit limiter #18 | `Engine::step` inside #14 | **next step**; reset to 1 there |
| `brakeSystem.electronicOverride` | pit limiter #18 | `BrakeSystem::step` #1 | **next step**; reset to 0 there |
| `speedLimiter.isLimiting` | pit limiter #18 | `Kers::step` #8, `ERS::step` #9 (no electric power while limiting) | next step |
| body torque | `StabilityControl::step` #27 | rigid-body step right after `Car::step` | none |
| `controls.gas`, `gearUp`, `gearDn` | AutoBlip #11, AutoShifter #12 | GearChanger #13, Drivetrain #14 | none; but brakes #1, KERS/ERS #8/#9 and steering #10 already read `controls` earlier in the step |
| `controls.clutch` | `Autoclutch::step` in `Car::step` | Drivetrain #14, AutoShifter #12 | none |

So a slip that appears in `Tyre::step` of step N is seen by ABS/TC in step N (if it is a check step)
and acts on the wheel/engine in step N+1.

### AI cars (matters for checking against an AI lap)

- `AIDriver::AIDriver`: `useAbs = abs.isActive && abs.isPresent`, then `abs.isActive = false`,
  `tractionControl.isActive = false`, `autoShift.isActive = false`, auto-clutch on start and on
  changes = true, `Drivetrain::forceAutoCutoffTime(0)`, `autoBlip.isActive = true`,
  `brakeSystem.brakePowerMultiplier = 1`.
- `AIDriver::acquireControls` every step: `stabilityControl.useBeta = true`; `gain = 2.0`, or 0.0 for
  1000 ms after the last collision. `AIDriver::stepHumanizer` 0x1402a19e0 sets `gain = 0` during a
  "traction mistake".
- `AIDriver::AIDriver` also replaces the auto-clutch downshift profile when the car's own one is slow:
  if `Autoclutch::getDownshiftSequenceDuration()` (the time of the profile's 4th point, 0 for an empty
  profile) is above 0.5 s, the profile becomes `(0, 1) (0.01, 0) (t, 0) (t * 2.5, 1)` with
  `t = Drivetrain::getGearDnTime()`.
- `AIDriver::stepSuperhuman` (called from `acquireControls`) is the AI's own ABS and its own throttle
  limiter. ABS part, per wheel, every step: `absOverride = 1`; then
  `if (useAbs or humanize.brakeMistakeTime <= 0) and status.ndSlip > 1.0 and (driven or load / (sum of the four loads) > 0.05): absOverride = 0`.
  The car's `ABS::step` still runs with
  `isActive == false` and writes `absOverride = 1` on its check steps, but that is after the tyres
  have used the AI's value and before the AI writes again in the next `pollControls`.
- The pit limiter and EDL work the same for AI cars.

### Cockpit controls: which code changes the levels

```
controls provider → Car::controls.absUp / tcDn / ... (Car::pollControls)
→ Car::getPhysicsState 0x140270d70 copies them into CarPhysicsState.actionsState bits:
    tcUp 0x800, tcDn 0x1000, absUp 0x2000, absDn 0x4000, turboUp 0x8000, turboDn 0x10000,
    engineBrakeUp 0x200000, engineBrakeDn 0x400000, MGUKDeliveryUp 0x800000, MGUKDeliveryDn 0x1000000,
    MGUKRecoveryUp 0x2000000, MGUKRecoveryDn 0x4000000, MGUHMode 0x8000000
→ (game thread) AbsNotifier::update 0x1400daf90 / TractionControlNotifier::update 0x1400dca70 / ...
    rising edge → AbsNotifier::action 0x1400d1c10 / TractionControlNotifier::action 0x1400d20e0
→ CarAvatar::cycleAbsMode 0x1400d2850 / cycleTcMode 0x1400d2d60 / cycleEngineBrake 0x1400d2c90 /
    cycleERSPower 0x1400d2a80 / cycleERSRecovery 0x1400d2bc0 / cycleERSHeatCharging 0x1400d29e0 /
    setTurboBoost 0x1400da9e0
→ PhysicsAvatar::executeOnPhysicsThread → next evOnPreStep
```

| Control | Physics-thread job | Effect |
|---|---|---|
| ABS level | lambda 0x1400d0b10 | `ABS::cycleMode(dir)` if `abs.isPresent` |
| TC level | lambda 0x1400d0b70 | `TractionControl::cycleMode(dir)` if `tractionControl.isPresent` |
| Engine brake | lambda 0x1400d0720 | `Engine::setCoastSettings(index)`; the index is kept on the game side and wraps over `engineBrakeSettingsCount` |
| MGU-K delivery | lambda 0x1400d08e0 | `ERS::setPowerController(index)`: copies `ersPowerControllers[index]` (and the front one) into the live `controller` |
| MGU-K recovery | lambda 0x1400d0570 | `ers.kineticRecovery = index * 0.1`; the index is kept on the game side, 0..10, wrapping both ways |
| MGU-H mode | lambda 0x1400d0ad0 | `ers.isHeatCharginBattery = flag` |
| Turbo boost | direct call (not queued), only when `engine.turboAdjustableFromCockpit` | `Engine::setTurboBoostLevel` 0x140288090 → every `Turbo::setTurboBoostLevel` |
| Options: ABS | lambda 0x1400d0bc0 (`CarAvatar::setAbsEnabled` 0x1400d9bc0) | `abs.isActive = on`; `abs.isPresent = on` when the second flag is set |
| Options: TC | lambda 0x1400d0a90 (`CarAvatar::setTcEnabled` 0x1400da950) | same for `tractionControl` |
| Options: stability | lambda 0x1400d0b40 (`CarAvatar::setStabilityControl` 0x1400da8c0) | `stabilityControl.gain = max(0, v)` |
| Options: auto blip | lambda 0x1400d07a0 (`CarAvatar::setAutoBlip` 0x1400d9c50) | `autoBlip.isActive`; off + shifter supported → `autoCutOffTime = 0` |
| Options: auto shifter | lambda 0x1400d09b0 (`CarAvatar::setAutoShifter` 0x1400d9d50) | `autoShift.isActive`; provider notified |
| Options: auto clutch | `CarAvatar::setAutoClutchEnabled` 0x1400d9ce0 | written directly, not queued |

The same cyclers are also called from `Sim::onKeyDown` 0x14019a940 (keyboard shortcuts) and from the
setup screen (`SetupElectronics` spinners; setup file sections `[ABS]`, `[TRACTION_CONTROL]`,
`[BRAKE_ENGINE]`, `[MGUK_DELIVERY]`, `[MGUK_RECOVERY]`, `[MGUH_MODE]`, `[TURBO]`, each with `VALUE`).
`ACPlugin::setABS` 0x1400bfb30, `ACPlugin::setTC` 0x1400bfdc0, `ACPlugin::setAutoShift` 0x1400bfb70 and
`ACPlugin::setStabilityControl` 0x1400bfc20 write the members directly (no queue): `setABS(v)` /
`setTC(v)` store `slipRatioLimit = v` and set `isActive = isPresent = (v > 0)`; `setAutoShift(b)` stores
`autoShift.isActive`; `setStabilityControl(v)` stores `gain = v` without the `max(0, v)` clamp.

### Telemetry (shared memory, `SharedMemoryWriter::updatePhysics` 0x140186ef0)

`tc` = `tractionControl.slipRatioLimit` if `isActive` else 0; `abs` = `abs.slipRatioLimit` if present
and active else 0; pit limiter flag = `speedLimiter.isLimiting`; auto-shifter flag =
`autoShift.isActive`. (Field names on the shared-memory side belong to the telemetry map.)

---

## 7. Port notes

**Effort: S for each aid, M for the whole area.** TC, ABS, EDL, stability control and the pit limiter
are 20 - 60 lines each with no hidden state beyond a timer. `DynamicController` is small too, but it
needs `Curve` (file and inline forms), 24 inputs that reach into tyres, drivetrain, suspension and
controls, and the per-stage filter state. The driver-edit aids (auto-blip, auto-shifter, auto-clutch)
are simple but depend on drivetrain details (gear requests, `isChangingGear`, engine rpm helpers).

Port before it: tyre (done), `Curve`, `CarControls`, the brake system (for `inputs.brakeTorque` and
`getOptimalBrake`), the engine (`electronicOverride`, coast settings) and the drivetrain (traction
type, `driven` flags, gears). `StabilityControl` needs only the rigid-body wrapper.

Order inside a ported `stepComponents` must be kept exactly (section 6): ABS/TC/limiter results are
consumed one step later, EDL the same step.

Bit-exact testing with an oracle (same technique as the tyre oracle: map `acs.exe`, bind the CRT
imports, call the game functions on hand-built objects):

| Target | Call | What to fake |
|---|---|---|
| TC | `TractionControl::step` 0x140290200 on a 0xa8 block | a `Car` block (0x3ea0 bytes) with `tyres[i].driven`, `tyres[i].status.slipRatio`, `valueCache.speed`, `drivetrain.acEngine.electronicOverride`, and `body` = an object with a fake `IRigidBody` vtable whose +0x78 (`getVelocity`) returns a chosen vector. Reset the override to 1 between calls to imitate the engine. |
| ABS | `ABS::step` 0x14028f610, `ABS::isInAction` | same `Car` block; check the four `absOverride` |
| EDL | `EDL::step` 0x1402bb460 | `tyres[i].status.angularVelocity`, `accG.z`, `inputs.brakeTorque` |
| Stability | `StabilityControl::step` 0x1402bfa50 | fake body with +0xb8 `getLocalVelocity`, +0x28 `getMass`, +0xe0 `addLocalTorque` (record the argument); needs `atanf` from the CRT |
| Levels | `TractionControl::cycleMode`, `ABS::cycleMode`, `getCurrentMode` | a real `Curve` in `valueCurve` (build it with the game's own `Curve` constructor and `Curve::load`, or run `TractionControl::init` / `ABS::init` with a fake `Car` that has `unixName` and a working `getConfigPath`) |
| Controller | `DynamicController::DynamicController(car, path)` 0x1402af330 then `DynamicController::eval` 0x1402b0c00 | temporary ini + lut files inside the repo; the `Car` block with the fields in section 5.9; for inputs 22 - 24 a fake `ISuspension` with virtual +0x48; `INIReader::useCache` must be 0 as in the tyre oracle |
| Pit limiter | `SpeedLimiter::step` 0x1402bb910 | real `Tyre` objects (it calls `Tyre::getDX` / `getCorrectedD`), `surfaceDef->isPitlane`, a `BrakeSystem` with `brakePower` and `frontBias` |
| Auto aids | `AutoBlip::step`, `AutoShifter::step`, `Autoclutch::step` | need a working `Drivetrain` / `Engine` (rpm helpers, gear vector) and `PhysicsEngine::physicsTime`; cheaper to test them inside a whole-car oracle |

Sequences matter more than single calls here: drive each aid with a few hundred steps of a slip
trace (for example taken from the tyre oracle) and compare the whole output trace, so the timer
behaviour (float32 accumulation of 0.003) is covered. Of the functions in this map only
`TractionControl::step` touches a static `perfCounter` guard on its first call (plain memory writes,
no imports); `ABS::step`, `EDL::step`, `StabilityControl::step`, `SpeedLimiter::step`,
`DynamicController::eval`, `AutoBlip::step`, `AutoShifter::step`, `Autoclutch::step` and
`GearChanger::step` have none.

---

## 8. Open questions

1. **Float accumulation of the timers.** The check periods in 5.1 come from a float32 emulation of
   the two instructions (`addss`, `comiss`) for every `RATE_HZ` in `cardata/`; they were not run in the
   game. An oracle trace should confirm them, especially `RATE_HZ` values where `1 / RATE_HZ` is close
   to a multiple of 0.003 (165 and 333 are the closest in `cardata/`).
2. **`TractionControl::lastValue` and `ABS::currentValue`** have no reader in the code I read
   (`currentValue` is only written as 1). Probably leftovers.
3. **`ABS` channel value outside 1 / 2 / 4** cannot come from the ini (fatal error), but `ACPlugin`
   or memory edits could produce it; then the active branch writes nothing. Not relevant for a port.
4. **`[ABS_V2]` `CURVE` quirk** (file name taken from `[ABS]`) is confirmed in the code but has no
   effect on any car in `cardata/`. I chose to document it rather than test it.
5. **Level table lookup** goes through linear interpolation. I simulated it in float32 for every table
   in `cardata/` and found no difference from the parsed values, but the port must still use the same
   formula (or prove equality) to be safe for other cars.
6. **Four cars name a `traction_control.lut` that is not in `cardata/`** (ks_ferrari_sf15t,
   ks_ferrari_sf70h, ks_maserati_levante, ks_maserati_quattroporte). Whether the file is missing in
   the game data too or was lost in extraction was not checked. `Curve::load` with a missing plain
   file leaves an empty curve, giving plain on/off (read by the reviewer, section 4.2); the packed
   `data.acd` path (`Curve::loadEncrypted`) was not read.
7. **Sign of the stability torque.** The formulas are exact, but I did not work out which way a
   positive local-Y torque turns the car or verify that it opposes the slide; the rigid-body map
   has the axis conventions.
8. **`Autoclutch::loadINI`** has now been read line by line by the reviewer (section 4.4 is confirmed).
   `Autoclutch::step` / the shift profiles overlap with the drivetrain map; the drivetrain document
   should be treated as the owner of clutch behaviour.
9. **`AutoBlip` time units.** `POINT_n` are used directly against `physicsTime - blipStartTime`, which
   I take to be milliseconds because `physicsTime` is compared with millisecond constants elsewhere
   (1000 in `AIDriver::acquireControls`). Not checked in a run.
10. **Gear numbering** (0 = reverse, 1 = neutral, 2 = first) is my interpretation from
    `AutoShifter::step` and the `GEAR` input. It agrees with `docs/map/drivetrain.md` (`gears[1]` has
    ratio 0) and with `Drivetrain::gearUp` 0x1402669f0 parking `currentGear` at 1 during a shift.
11. **Kers controller** (`[KERS] CONTROLLER` in kers.ini) and the ERS controller lists were located
    but not analysed; number of cars not counted. They belong to the drivetrain / hybrid map.
12. **Online**: `ServerDrivingAssists` overrules the local options at session start; whether the
    server can change them mid-session was not looked at.
13. **Does each controller get evaluated exactly once per step?** True for every call site read.
    `ERS::step` contains exactly two `eval` calls, one for `controller` and one for `controllerFront`,
    back to back in the same branch (each result clamped to 0..1), so neither filter advances twice.
    Not checked: whether that branch is skipped in some ERS states (then the filter would not advance
    at all in those steps). `BrakeSystem::step` calls `eval` for `ebbController` only when
    `ebbMode == DynamicController`, and for `steerBrake.controller` only when `steerBrake.isActive`.
14. **`DynamicController` inputs for AWD_NEW cars**: `SLIPRATIO_MAX` / `SLIPRATIO_AVG` return 0 for
    traction type 3 according to the pseudo-C. This looks like an oversight in the game; I report it
    as read.
15. **escmode.ini purpose.** Confirmed: only `Sim::Sim` reads it and the values go to
    `CameraOnBoardFree`. Unverified: that this camera is the Esc / pause-menu view (taken from the
    file name only).
16. **Choices made**: I treated auto-blip / auto-shifter / auto-clutch / gear changer as part of this
    map only as far as the assist switches go, and gave their step maths in compact form; I did not
    map `Engine::step`, `Drivetrain`, `ERS` or `Kers` beyond the lines that touch electronics.

---

## 9. System-specific answers

- **`[SPEED_LIMITER]`, `[AUTOBLIP]` in electronics.ini?** No. The pit limiter has no data at all
  (80 / 81 km/h literals). `[AUTOBLIP]` is in drivetrain.ini.
- **`DISPLAY_SLIP_RATIO_LIMIT`?** Not in the executable.
- **What is escmode.ini?** A camera position and field of view for `CameraOnBoardFree` (by its name the
  pause-menu view; that part is unverified). Not stability control, not physics.
- **What does TC write?** `engine.electronicOverride = 0` (full throttle cut), nothing else.
- **What does ABS write?** `tyres[i].absOverride` = 0 or 1.
- **What does EDL write?** Extra `inputs.brakeTorque` on the faster driven wheel.
- **What does stability control apply?** A yaw torque on the main body (`addLocalTorque`, local Y).
  No brake or engine intervention.
- **Engine maps?** There is no engine-map selector. The only engine setting switched from the cockpit
  is the engine-brake level (`[COAST_SETTINGS]`, 2 cars) and the turbo boost level.
