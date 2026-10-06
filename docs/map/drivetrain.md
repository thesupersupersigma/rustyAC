# Engine and drivetrain

Source: `acs.exe` + `acs.pdb`, read only through the local material: Ghidra pseudo-C in
`re/decomp/{Engine,Turbo,Drivetrain,Autoclutch,AutoBlip,AutoShifter,GearChanger,Kers,ERS,DynamicController}/`,
struct layouts in `re/types/`, `tools/re_query.py`, `tools/disasm.py` (for constants and operation
order of `Drivetrain::step2WD`, `Engine::step`, `ERS::step`) and `tools/ini_survey.py` over the 113
cars in `cardata/`. Raw copies of every function cited here: `re/car/drivetrain/` (plus two
disassembly listings, `step2WD.asm` and `engine_step.asm`).
Nothing in the game folder, the Ghidra project or git was changed. No Rust was written.

Size of the area: 113 hand-written functions, about 76 KB of code
(Drivetrain 22.8 KB, Engine 16.2 KB, DynamicController 11.6 KB, ERS 9.9 KB, Autoclutch 6.1 KB,
Kers 4.2 KB, AutoShifter 3.1 KB, AutoBlip 2.1 KB, Turbo 0.3 KB, GearChanger 0.2 KB).
More than half of that is ini loading.

Marking used below: **[code]** = read directly in the pseudo-C or disassembly;
**[interp]** = my interpretation of what the code means.

---

## 1. Plain-English summary

Each physics step the car first decides how much throttle the engine really gets: the driver's
pedal is reshaped by a throttle curve, then forced to zero by the rev limiter, by a gear-change
cut, by traction control or by the pit speed limiter.
The engine then looks up its full-throttle torque for the current revs, multiplies it by the
turbo boost, and blends it with the engine-braking torque according to the throttle; hot air
makes it a little weaker.
The turbo is a single number between 0 and 1 that chases a target set by revs and throttle,
quickly or slowly depending on two lag settings, and is capped by the wastegate.
The clutch is a friction link between the engine and the gearbox: when the pedal is fully up and
both sides turn at nearly the same speed they are treated as one solid piece, otherwise the two
sides spin separately and the clutch drags them towards each other with a limited torque.
The gearbox is only a ratio; a paddle shift puts the box in neutral for the shift time and then
drops the new gear in, at which moment engine and wheels are brought to a common speed that
respects their inertias.
The driven wheels are not spun up by the tyre code: the tyre only reports the net torque acting
on each driven wheel (road reaction, brake, rolling drag), and the drivetrain adds the engine
torque, spins the whole block of engine, gearbox and wheels together, and then lets the left and
right wheel differ by an amount the differential allows.
The differential resists a speed difference with a torque that grows with the engine torque
(separately for power and coast) plus a fixed preload; four-wheel-drive cars have a front, a rear
and a centre differential, or a rear-drive layout with a clutch that sends torque to the front.
Small helper systems press the clutch for you when pulling away and during shifts, blip the
throttle on downshifts, shift automatically, and refuse downshifts that would over-rev.
Hybrid cars have a battery that charges under braking and engine braking (and from turbo heat)
and empties when a controller, or the driver's button, asks for electric torque, which is added
to the engine torque or to the driven wheels.
Over-revving and too much boost slowly use up the engine's life; a dead or fuel-less engine
only produces drag.

---

## 2. Classes

Sizes from `re/types/<Type>.txt`.

| Class | Size | What it is for |
|---|---|---|
| `Drivetrain` | 0x648 | Owns the engine, clutch, gearbox and differentials. Integrates engine speed and the angular speed of the driven wheels. Member `Car::drivetrain` (Car+0x310). |
| `GearElement` | 0x18 | One rotating part: `velocity` (rad/s, double), `inertia`, `oldVelocity`. Six of them: `engine`, `drive` (differential carrier), `outShaftL/R` (rear, or the driven axle on FWD), `outShaftLF/RF` (front, 4WD only). |
| `SGearRatio` | 0x28 | One gear: `ratio` (double) and a name. `Drivetrain::gears` is a vector of these. |
| `GearRequestStatus` | 0x20 | Pending sequential shift: `request` (0 none, 1 up, 2 down), `timeAccumulator`, `timeout`, `requestedGear`. |
| `DifferentialSetting` | 0x10 | `power`, `coast`, `preload`, `type` for the three AWD differentials. |
| `AWD2Data` | 0x18 | Centre coupling of the `AWD2` layout: `ramp`, `maxTorque`, `currentLockTorque`. |
| `DownshiftProtection` | 0xc | `isActive`, `isDebug`, `overrev`, `lockN`. |
| `DrivetrainControllers` | 0x20 | Four optional `DynamicController` pointers: `awdFrontShare`, `awdCenterLock`, `singleDiffLock`, `awd2`. |
| `Engine` | 0x3e8 | Torque source. Member `Drivetrain::acEngine`. Has a vtable: +0x08 `getLimiterRPM`, +0x10 `isLimiterOn`. |
| `acEngineData` | 0x128 | `powerCurve` (really a torque curve, Nm vs rpm), `coast0/1/2`, `minimum` (idle rpm), `limiter`, `limiterCycles`, overlap settings. `coastCurve`, `useCoastCurve`, `coast0` are not used by the step. |
| `EngineStatus` | 0x18 | Outputs: `outTorque` (double, Nm), `externalCoastTorque`, `turboBoost`, `isLimiterOn`. |
| `SACEngineInput` | 0x10 | Input of `Engine::step`: `gasInput`, `carSpeed`, `altitude`, `rpm`. Only gas and rpm are filled by the caller. |
| `Turbo` / `TurboDef` | 0x24 / 0x1c | One turbo: `rotation` (0..1 state), `userSetting` (cockpit boost level), and its data `maxBoost`, `lagUP`, `lagDN`, `rpmRef`, `gamma`, `wastegate`, `isAdjustable`. |
| `TurboDynamicController` | 0x38 | A `DynamicController` bound to one turbo; drives its `maxBoost` or (if `isWastegate`) its `wastegate`. |
| `PushToPass` | 0x28 | Overboost button state (`Engine::p2p`). |
| `ITorqueGenerator`, `ICoastGenerator` | 0x8 | One-method interfaces (vtable +0x08) used to add electric torque / coast torque. Implemented by `Kers` and `ERS`. |
| `DynamicController` | 0x28 | Generic "inputs through look-up tables" calculator defined by a `ctrl_*.ini` file. Used here by diff lock, AWD, turbo, KERS, ERS (and elsewhere by brakes, anti-roll bars, steering). |
| `DynamicControllerStage` | 0xa0 | One `[CONTROLLER_n]` stage: `inputVar`, `combinatorMode`, `lut`, `filter`, `upLimit`, `downLimit`, `currentValue`, `constValue`. |
| `Autoclutch` | 0x1a8 | Presses/releases `Car::controls.clutch` at low revs and during shifts. `ClutchSequence` (0x88) is one timed clutch profile being played. |
| `AutoBlip` | 0xa8 | Raises `Car::controls.gas` for a short time after a downshift request. |
| `AutoShifter` | 0x28 | Automatic gearbox assist: sets `Car::controls.gearUp/gearDn`. |
| `GearChanger` | 0x18 | Turns the gear controls into `Drivetrain::gearUp/gearDown/setCurrentGear` calls. |
| `Kers` | 0xf8 | Simple one-motor hybrid (`kers.ini`). `ITorqueGenerator`. |
| `ERS` | 0x2b0 | Modern hybrid: MGU-K, MGU-H, optional front motors (`ers.ini`). `ITorqueGenerator` + `ICoastGenerator`. `ERSPowerController` (0x48) is one named delivery map. |
| `ThermalObject` | 0x20 | `Car::water`: the water temperature, a display value only. |

Enum values (from compare constants in the disassembly) **[code]**:
`TractionType` RWD 0, FWD 1, AWD 2, AWD_NEW 3. `DifferentialType` LSD 0, Spool 1.
`GearChangeRequest` none 0, up 1, down 2. `KersAttachment` Engine 0, Wheels 1.
`TorqueModeEX` original 0, reactionTorques 1.

---

## 3. Key functions

### 3.1 Loading / init (once per car, from `Car::Car` 0x14026bf00)

```
Car::Car                                         0x14026bf00
├─ Drivetrain::Drivetrain (member ctor, defaults) 0x14026d870
│   └─ Engine::Engine 0x1402852b0 → acEngineData::acEngineData 0x1402854a0
├─ ERS::ERS (defaults)                            0x140291500
├─ ERS::init                                      0x140291c30   ers.ini, ctrl_ers_N.ini, ctrl_ers_front_N.ini
│   └─ ERS::setPowerController                    0x140292fc0
├─ Kers::init  (only if ers.present is false)     0x1402b7360   kers.ini
├─ Drivetrain::init                               0x140266dc0
│   ├─ Engine::init                               0x140285e10
│   │   ├─ Engine::loadINI                        0x140286100   engine.ini, power.lut, throttle.lut, ctrl_turboN.ini, ctrl_wastegateN.ini
│   │   │   ├─ Engine::loadCoastSettings          0x140285ea0   [COAST_REF] → coast1, coast2
│   │   │   ├─ Engine::setCoastSettings           0x140288010   [COAST_SETTINGS]
│   │   │   ├─ Turbo::Turbo                       0x1402ae730
│   │   │   ├─ Engine::setTurboBoostLevel         0x140288090 → Turbo::setTurboBoostLevel 0x1402ae7a0
│   │   │   └─ DynamicController::DynamicController 0x1402af330
│   │   ├─ Engine::reset                          0x140287f90   turbos to 0, lifeLeft = 1000
│   │   └─ Engine::precalculatePowerAndTorque     0x140287d10   max torque / max power and their rpm
│   ├─ Drivetrain::loadINI                        0x140267440   drivetrain.ini, ctrl_awd2.ini
│   │   ├─ Drivetrain::addGear                    0x140266570
│   │   └─ Drivetrain::setCurrentGear(1, true)    0x1402692e0   start in neutral
│   ├─ Engine::addTorqueGenerator 0x140285a10 / Engine::addCoastGenerator 0x1402859f0   (KERS on engine, ERS)
│   │   or push to Drivetrain::wheelTorqueGenerators                                    (KERS on wheels)
│   └─ Drivetrain::initControllers                0x140267070   ctrl_single_lock.ini, ctrl_awd_front_share.ini, ctrl_awd_center_lock.ini
├─ Autoclutch::init                               0x1402b87d0 → Autoclutch::loadINI 0x1402b8890  (+ gear-request handler)
├─ AutoBlip::init                                 0x1402b9910 → AutoBlip::loadINI 0x1402b99b0    (+ gear-request handler)
├─ AutoShifter::init                              0x1402ba020 → AutoShifter::loadINI 0x1402ba050
└─ GearChanger::init                              0x1402bab40
```

Later, from the setup screen: `SetupManager::initItems` 0x140289570 binds setup items straight to
drivetrain members (section 4.6), gear ratios go through `Drivetrain::setGearRatio` 0x140269470.

### 3.2 Every physics step

```
Car::step                                         0x140275da0
├─ fuel burn from Engine::gasUsage, sets Engine::fuelPressure          (inline, section 5.9)
├─ Autoclutch::step                               0x1402b9590   writes controls.clutch
│   └─ Autoclutch::stepSequence                   0x1402b97a0   (while a shift profile is playing)
├─ Car::stepThermalObjects                        0x1402769f0   water temperature (display only)
└─ Car::stepComponents                            0x1402764d0
    ├─ BrakeSystem::step, EDL::step, suspensions, Tyre::step x4   → status.feedbackTorque of each wheel
    ├─ ... HeaveSpring, DRS, AeroMap ...
    ├─ Kers::step  (if kers.present)              0x1402b7e10
    ├─ ERS::step   (if ers.present)               0x1402930e0
    │   └─ DynamicController::eval x2             0x1402b0c00 → DynamicController::getInput 0x1402b0d70
    ├─ SteeringSystem::step
    ├─ AutoBlip::step                             0x1402b9ef0   may raise controls.gas
    ├─ AutoShifter::step                          0x1402ba7f0   may set controls.gearUp / gearDn, cut gas
    ├─ GearChanger::step                          0x1402bab50
    │   ├─ Drivetrain::gearUp                     0x1402669f0   → evOnGearRequest handlers
    │   ├─ Drivetrain::gearDown                   0x140266660   → evOnGearRequest handlers:
    │   │       Autoclutch::onGearRequest 0x1402b9350, AutoBlip handler 0x1402b9880
    │   └─ Drivetrain::setCurrentGear             0x1402692e0   (H-shifter path)
    │       all three first ask Drivetrain::isGearboxLocked 0x1402673c0
    ├─ Drivetrain::step                           0x14026b130
    │   ├─ Drivetrain::stepControllers            0x14026b200   DynamicController::eval
    │   └─ one of:
    │      Drivetrain::step2WD (RWD, FWD)         0x1402694e0
    │      │  ├─ Drivetrain::getCurrentGear       0x140266b10
    │      │  ├─ wheelTorqueGenerators[i]->getOutputTorque   (Kers::getOutputTorque 0x1402b72b0)
    │      │  ├─ Drivetrain::reallignSpeeds       0x140269100   (only when the ratio changed)
    │      │  │    └─ Drivetrain::getInertiaFromEngine 0x140266ba0, accelerateDrivetrainBlock
    │      │  ├─ Engine::step                     0x1402880e0
    │      │  │    ├─ Engine::getThrottleResponseGas 0x140285b90
    │      │  │    ├─ Engine::stepP2P             0x140288750   (push-to-pass, if enabled)
    │      │  │    ├─ Curve::getValue(powerCurve, rpm)
    │      │  │    ├─ Engine::stepTurbos          0x140288900
    │      │  │    │    ├─ DynamicController::eval (turbo controllers)
    │      │  │    │    └─ Turbo::step 0x1402ae7c0, Turbo::getBoost 0x1402ae780
    │      │  │    ├─ coastGenerators[i]->getCoastTorque   (ERS::getCoastTorque 0x140291b40)
    │      │  │    ├─ PhysicsEngine::getAirDensity 0x140263a60
    │      │  │    └─ torqueGenerators[i]->getOutputTorque (ERS::getOutputTorque 0x140291bd0, Kers::getOutputTorque)
    │      │  ├─ Drivetrain::getInertiaFromWheels 0x140266c20
    │      │  ├─ Drivetrain::accelerateDrivetrainBlock 0x1402664c0
    │      │  ├─ Tyre::stepRotationMatrix x2      0x140284b80
    │      │  └─ body / rigid axle addLocalTorque (+0xe0) or addTorque (+0x118)   reaction torques
    │      Drivetrain::step4WD (AWD)              0x14026a220   same skeleton, three differentials, 4 wheels
    │      Drivetrain::step4WD_new (AWD2)         0x14026ad80   = step2WD on the rear axle + centre coupling
    ├─ AntirollBar x2, ABS::step
    ├─ TractionControl::step, SpeedLimiter::step   set Engine::electronicOverride = 0 (used next step)
    └─ ...
```

Other callers worth knowing: `Car::forcePosition` 0x14026fe10 calls `Drivetrain::reset`
0x140269280 (all six shaft speeds and `rootVelocity` to 0, `clutchOpenState = true`, pending shift
dropped, `validShiftRPMWindow = orgRpmWindow` — which repairs gearbox wear, and is the raw ini
value, so 0 when the key is missing — then `Engine::reset`) and afterwards
`Drivetrain::setCurrentGear(1, true)`; `Car::onCollisionCallBack` 0x140274650 calls
`Engine::blowUp` 0x140285a30 when the closing speed along the contact normal, in km/h, times
`mechanicalDamageRate` is above 150 **[code]** (two tests on the colliding shapes in front of it
were not interpreted; they belong to the body map);
`AIDriver::stepGears` 0x1402a1710 uses `Drivetrain::projectRPMAtDownshift` 0x140268ee0
(`(float)((wR + wL) · 0.5) · gears[current − 1].ratio · finalRatio · 0.15915507 · 60`, or −1 in
reverse or when the lower gear is neutral);
`RaceEngineer` uses `Engine::getTorqueAtRPM` 0x140285d10 for estimates (not part of the step).
`Car::reset` 0x1402758e0 and `TimeTransponder::onTimeLinePassed` 0x140290c20 (branch
`lapCount == 0 && isFirstLapArmed`) call `Kers::reset` 0x1402b7e00 / `ERS::reset` 0x140292fa0.

Small accessors with behaviour **[code]**:
`Engine::getMaxTurboBoost(useWastegate)` 0x140285af0 = Σ over turbos of `maxBoost`, or of
`min(maxBoost, wastegate)` when the flag is set and `wastegate != 0` (`ERS::step` passes false);
`Engine::getSafeTurboLevel` 0x140285b50 = `turboBoostDamageThreshold / Σ maxBoost` (used by
`AIDriver::stepTurbo`);
`Engine::getTurboBoostLevel` 0x140285dc0 = `turbos[0].userSetting` (0 without turbo);
`Engine::getMaxPowerW` 0x140285a80 = `maxPowerW_Dynamic` if it is > 0, else
`(Σ maxBoost + 1) · maxPowerW`;
`Drivetrain::getRpmWindowStatus` 0x140266d70 = `validShiftRPMWindow / orgRpmWindow` (gearbox
health for `Car::getPhysicsState`); `Drivetrain::isChangingGear` 0x1402673b0 =
`gearRequest.request != none` (AI and `AutoShifter`);
`Kers::getDischargeTimeS` 0x1402b7290 = `1 / dischargeK` seconds (`AIDriver::AIDriver`);
`Drivetrain::getDrivetrainSpeed` 0x140266b70 = `(float)drive.velocity` (`Car::getPhysicsState`);
`Engine::getMaxPowerRPM` 0x140285a70, `Engine::getMaxTorqueRPM` 0x140285ae0 and
`Engine::getMaxTorqueNM` 0x140285ad0 return what `Engine::precalculatePowerAndTorque` found: it
scans `powerCurve` from 0 rpm in 50 rpm steps up to the last table rpm, keeps the largest torque
and the largest `rpm · torque · 0.1047` (W), both without boost (used by `AutoShifter::step`, AI).

---

## 4. Data it reads

All files are in `content/cars/<car>/data/`. "n/113" = how many of the 113 extracted cars have
the key (`tools/ini_survey.py`). `INIReader::getFloat/getInt` return 0 for a missing key, so
"default" below is what the code does with 0, or the constructor value when the loader is skipped.

### 4.1 engine.ini → `Engine` (`Engine::loadINI` 0x140286100)

| Section / key | Member | Notes |
|---|---|---|
| `[HEADER] VERSION` | (only printed) | 1 in 111 cars, 2 in 2. No version switch in the loader. |
| `[HEADER] POWER_CURVE` | `data.powerCurve` (file name) | `power.lut`, rpm → **torque in Nm** at full throttle, without boost. |
| `[HEADER] COAST_CURVE` | — | Only the value `FROM_COAST_REF` does anything (113/113). With any other value `coast1`/`coast2` keep the `acEngineData` ctor values 0 and **1e-6** (0x1402854a0), i.e. a small quadratic coast torque. |
| `[ENGINE_DATA] MINIMUM` | `data.minimum` (int rpm) | Idle. 0 → 1000. |
| `[ENGINE_DATA] INERTIA` | `inertia` (kg·m²) | Copied to `Drivetrain::engine.inertia` every step. Ctor default 1.0. |
| `[ENGINE_DATA] LIMITER` | `data.limiter`, `defaultEngineLimiter` (int rpm) | 0 = no limiter. |
| `[ENGINE_DATA] LIMITER_HZ` | `data.limiterCycles` | Stored as a number of physics steps: `(1000 / HZ) / 3` (integer divisions). 0 → 50 steps. |
| `[ENGINE_DATA] DEFAULT_TURBO_ADJUSTMENT` | `Turbo::userSetting` of every adjustable turbo | Read only if some turbo is cockpit adjustable. 17/113. |
| `[ENGINE_DATA] ALTITUDE_SENSITIVITY` | — | **Not read by the game** (113 cars have it). |
| `[COAST_REF] RPM, TORQUE, NON_LINEARITY` | `data.coast1`, `data.coast2` | `a = (1 − NL)·RPM − minimum`, `b = NL·RPM`; `coast1 = a ≠ 0 ? −TORQUE / a : 0`; `coast2 = b ≠ 0 ? TORQUE / b² : 0`. `NON_LINEARITY` is 0 in all 113 cars, so `coast2 = 0`. |
| `[COAST_DATA]`, `[COAST_CURVE]` | — | **Not read** (the SDK example calls them obsolete). |
| `[COAST_SETTINGS] LUT, DEFAULT, ACTIVATION_RPM` | `gasCoastOffsetCurve`, `coastSettingsDefaultIndex`, `coastEntryRpm = ACTIVATION_RPM + minimum` | Cockpit "engine brake" levels; `gasCoastOffset = LUT(index)`. 2/113. |
| `[TURBO_n] LAG_UP, LAG_DN` | `TurboDef::lagUP, lagDN` | Stored as `(1 − LAG) · 1.3333334 · 333.33334` (a rate per second). 42/113 cars have at least `[TURBO_0]`. |
| `[TURBO_n] MAX_BOOST, WASTEGATE, REFERENCE_RPM, GAMMA` | `maxBoost`, `wastegate`, `rpmRef`, `gamma` | Boost is a torque multiplier minus one. Wastegate 0 = none. |
| `[TURBO_n] COCKPIT_ADJUSTABLE` | `isAdjustable`; sets `Engine::turboAdjustableFromCockpit` | |
| `[TURBO_n] DISPLAY_MAX_BOOST` | — | Not read by the physics loader (display apps). |
| `[OVERLAP] FREQUENCY, GAIN, IDEAL_RPM` | `data.overlapFreq/Gain/IdealRPM` | Camshaft-overlap roughness. 2/113, all zero. |
| `throttle.lut` (fixed name, if the file exists) | `throttleResponseCurve` | Pedal % → throttle %. 113/113. |
| `[THROTTLE_RESPONSE] RPM_REFERENCE, LUT` | `throttleResponseCurveMaxRef`, `throttleResponseCurveMax` | Second pedal map reached at the reference rpm. 5/113. |
| `[DAMAGE] TURBO_BOOST_THRESHOLD, TURBO_DAMAGE_K` | `turboBoostDamageThreshold`, `turboBoostDamageK` | Read only when the car has a turbo. |
| `[DAMAGE] RPM_THRESHOLD, RPM_DAMAGE_K` | `rpmDamageThreshold`, `rpmDamageK` | Without a `[DAMAGE]` section: threshold = `LIMITER · 1.05`, K = 10. |
| `[BOV] PRESSURE_THRESHOLD` | `bovThreshold` | Blow-off valve flag for sound. Default 0.2. 3/113. |
| `ctrl_turboN.ini`, `ctrl_wastegateN.ini` (if the file exists) | `turboControllers` | Controller for `maxBoost` / `wastegate` of turbo N. `ctrl_turbo0.ini` 6/113, `ctrl_turbo1.ini` 2/113, wastegate 0/113. |
| `[PUSH_TO_PASS] COOLDOWN_SECONDS, TIME_SECONDS, OVERBOOST, ACTIVATION_BASE, ACTIVATION_POS, ACTIVATION_MAX` | `p2p.*` | Needs a turbo. 0/113. |

There is no `[THROTTLE_LUT]` section in the loader; the throttle map is the file `throttle.lut`.
Keys found in a few mod cars and never read by acs.exe: `POWER_CURVE_STANDARD`, `HUMIDITY_REF`,
`TURBO_VERSION`, `AMBIENT_EFFECT`, `MECHANICAL_EFFICIENCY`, `IDLE_THROTTLE`, `IDLE_CUTOFF`,
`[FUEL_CONSUMPTION]`, `FLOW_ON_CUT`, `EXT_GAS_CURVE` (they belong to a third-party extension).

### 4.2 drivetrain.ini → `Drivetrain` (`Drivetrain::loadINI` 0x140267440)

| Section / key | Member | Notes |
|---|---|---|
| `[HEADER] VERSION` | — | 3 in all 113 cars. `> 1` enables `[DAMAGE]`; `> 2` enables `[AUTOCLUTCH] FORCED_ON`. |
| `[TRACTION] TYPE` | `tractionType`, `tyreLeft/Right`, `Tyre::driven` | `RWD` 94, `AWD2` 9, `FWD` 5, `AWD` 5. Anything else: error "TRACTION TYPE NOT FOUND". `Tyre::driven` is set for tyres 0,1 (`FWD`), tyres 2,3 (`RWD`) or all four (`AWD` and `AWD2`); the tyre, traction control, the AI and `DynamicController` all branch on that flag, so on an `AWD2` car the tyre never integrates the front wheel speeds itself (`step4WD_new` does). |
| `[GEARS] COUNT, GEAR_R, GEAR_1..n, FINAL` | `gears`, `finalRatio` | See 4.7 for the index convention. |
| `[DIFFERENTIAL] POWER, COAST, PRELOAD` | `diffPowerRamp`, `diffCoastRamp`, `diffPreLoad` (Nm) | `POWER ≥ 1` and `COAST ≥ 1` → `diffType = Spool`. Used by RWD, FWD. For AWD2 the Spool test is still made on these `[DIFFERENTIAL]` values; only afterwards do the `[AWD2] REAR_DIFF_*` keys overwrite the three members. |
| `[AWD] FRONT_SHARE` | `awdFrontShare = value · 0.01` | 14/113 have the section, 5 use it. |
| `[AWD] FRONT_DIFF_*, REAR_DIFF_*, CENTRE_DIFF_*` (`POWER/COAST/PRELOAD`) | `awdFrontDiff`, `awdRearDiff`, `awdCenterDiff` | |
| `[AWD2] FRONT_DIFF_POWER/COAST/PRELOAD` | `awdFrontDiff` | Loaded but **not used by `step4WD_new`** (front axle is open). |
| `[AWD2] REAR_DIFF_POWER/COAST/PRELOAD` | `diffPowerRamp`, `diffCoastRamp`, `diffPreLoad` | Overwrite the `[DIFFERENTIAL]` values. |
| `[AWD2] CENTRE_RAMP_TORQUE, CENTRE_MAX_TORQUE` | `awd2.ramp`, `awd2.maxTorque` | Ctor defaults 20 and 800. |
| `ctrl_awd2.ini` (if exists) | `controllers.awd2` | Drives `awd2.maxTorque`. 8/113. |
| `[GEARBOX] CHANGE_UP_TIME, CHANGE_DN_TIME` (ms) | `gearUpTime`, `gearDnTime` (s) | `· 0.001`. 0 → 0.1 s / 0.15 s. |
| `[GEARBOX] AUTO_CUTOFF_TIME` (ms) | `autoCutOffTime` (s) | 0 = no cut. |
| `[GEARBOX] SUPPORTS_SHIFTER` | `isShifterSupported` | 57/113 are 1. |
| `[GEARBOX] VALID_SHIFT_RPM_WINDOW` | `validShiftRPMWindow`, `orgRpmWindow` | 0 → 500 (but `orgRpmWindow` keeps the raw 0). |
| `[GEARBOX] CONTROLS_WINDOW_GAIN` | `controlsWindowGain` | |
| `[GEARBOX] INERTIA` | `clutchInertia` **and** `drive.inertia` | Only if non-zero. Defaults: `clutchInertia` 1.0 (ctor), `drive.inertia` 0.01 (init). |
| `[CLUTCH] MAX_TORQUE` | `clutchMaxTorque` (Nm) | 0 → 450. |
| `[DOWNSHIFT_PROTECTION] ACTIVE, DEBUG, OVERREV, LOCK_N` | `downshiftProtection.*` | Read only if the section exists **and** the car has no H-shifter support; otherwise `isActive = !isShifterSupported` and the ctor values `overrev = 0`, `lockN = true` stay. In **both** cases the loader then sets `isDebug = (physicsGUID == 0)` unconditionally, so the `DEBUG` key is read and immediately overwritten (no effect). 28/113. |
| `[DAMAGE] RPM_WINDOW_K` | `damageRpmWindow` | 69/113. |
| `ctrl_single_lock.ini` (RWD only, if exists) | `controllers.singleDiffLock` | 12/113 ship the file, but `Drivetrain::initControllers` opens it only when `tractionType == RWD`: 2 of the 12 are `AWD2` cars (`ks_porsche_cayenne`, `ks_porsche_macan`) where it is never loaded, so 10 cars really use it. FWD cars cannot have one either. |
| `ctrl_awd_front_share.ini`, `ctrl_awd_center_lock.ini` (AWD only) | `controllers.awdFrontShare`, `controllers.awdCenterLock` | 0/113 and 1/113. |

Side effects of the loader **[code]**: wheel inertias are copied from `Tyre::data.angularInertia`
into the out-shafts; for `AWD` the four tyre inertias are first **replaced by
`(tyre[0] + tyre[2]) / 2`**. Init defaults before the ini: RWD, `tyreLeft = tyres[2]`,
`tyreRight = tyres[3]`, `finalRatio` 4, diff 0.7 / 0.2 / 0, `awdFrontShare` 0.3.

The loader has no `[DOWNSHIFT_PROFILER]` section; the names asked for in the brief map to
`[DOWNSHIFT_PROFILE]` (autoclutch, below) and `[DOWNSHIFT_PROTECTION]`.

### 4.3 drivetrain.ini → helpers

| Reader | Section / key | Member |
|---|---|---|
| `Autoclutch::loadINI` 0x1402b8890 | `[AUTOCLUTCH] UPSHIFT_PROFILE`, `DOWNSHIFT_PROFILE` | Name of a section, or `NONE`. That section's `POINT_0/1/2` (ms, `· 0.001`) build a 4-point curve `(0,1) (P0,0) (P1,0) (P2,1)` = clutch value over time. Upshift profile used by 21/113, downshift by 112/113. |
| | `[AUTOCLUTCH] USE_ON_CHANGES` | `useAutoOnChange` |
| | `[AUTOCLUTCH] MIN_RPM, MAX_RPM` | `rpmMin`, `rpmMax`; if either is 0 → 1500 / 2500 |
| | `[AUTOCLUTCH] FORCED_ON` (VERSION > 2) | `isForced` (34/113) |
| `AutoBlip::loadINI` 0x1402b99b0 | `[AUTOBLIP] LEVEL, POINT_0/1/2` (ms) | `blipProfile` = `(0,0) (P0,LEVEL) (P1,LEVEL) (P2,0)`; `blipPerformTime` = P2 |
| | `[AUTOBLIP] ELECTRONIC` | `isElectronic` (57/113) |
| `AutoShifter::loadINI` 0x1402ba050 | `[AUTO_SHIFTER] UP, DOWN, SLIP_THRESHOLD, GAS_CUTOFF_TIME` | `changeUpRpm`, `changeDnRpm`, `slipThreshold`, `gasCutoffTime` (s). 108/113. Without the section the same keys are read from `ai.ini [GEARS]`. |

### 4.4 kers.ini → `Kers` (`Kers::init` 0x1402b7360) — 2/113 cars

`[HEADER] VERSION`; `[KERS] BRAKE_LEVEL` → `brakeForMaxCharge = BrakeSystem::getBrakePower() ·
BRAKE_LEVEL · 2`; `CHARGE_K` → `chargeK`; `TORQUE_CURVE` (file, rpm → Nm) → `torqueLUT`;
`DISCHARGE_TIME` (ms) → `dischargeK = 1000 / value`; `NEGATIVE_INPUT_CHARGE_K` (VERSION > 1);
`CONTROLLER` (a controller file name; empty = button only); `ATTACH` = `ENGINE` or `WHEELS`;
`HAS_BUTTON_OVERRIDE`, `MAX_KJ_PER_LAP` → `maxJ = value · 1000` (VERSION > 2).
`[KERS]` lives in `kers.ini`, not in engine.ini or drivetrain.ini.
`Kers::init` is skipped when the car has a working `ers.ini`.
Both shipped `kers.ini` files say `ATTACH=ENGINE` (so the `WHEELS` path of 5.2 step 3 is not
exercised by any of the 113 cars); one names `controller_kers.ini`, the other has an empty
`CONTROLLER` (button only).

### 4.5 ers.ini → `ERS` (`ERS::init` 0x140291c30) — 5/113 cars

| Section / key | Member |
|---|---|
| `[KINETIC] CHARGE_K` | `chargeK = value · 0.001` |
| `[KINETIC] TORQUE_CURVE`, `COAST_CURVE` | `torqueLUT`, `coastLUT` (rpm → Nm) |
| `[KINETIC] DISCHARGE_TIME` (ms) | `dischargeK = 1000 / value` |
| `[KINETIC] HAS_BUTTON_OVERRIDE`, `MAX_KJ_PER_LAP` | `hasButtonOverride`, `maxJ = value · 1000` |
| `[KINETIC] DEFAULT_CONTROLLER` | `defaultPowerControllerIndex` |
| `[KINETIC] BRAKE_REAR_CORRECTION` | `rearCorrectionTorque` |
| `[HEAT] CHARGE_K`, `TORQUE_PERC` | `heatChargeK`, `heatTorque = value · 0.01` |
| `[COCKPIT_CONTROLS] DELIVERY_PROFILE, MGU_H_MODE, RECOVERY` | `cockpitControls.*` (which knobs the HUD offers) |
| `[FRONT_MOTORS] TORQUE_CURVE, DISCHARGE_TIME, FRONT_TORQUE_VECTORING_BIAS` | `frontTorqueLUT`, `dischargeKFront = 1000 / value`, `frontTorqueVectoringBias = saturate(value)` (clamped to 0..1). Error message if the number of `ctrl_ers_front_N` differs from the number of `ctrl_ers_N`. (0/113) |
| `ctrl_ers_0.ini`, `ctrl_ers_1.ini`, … until a file is missing | `ersPowerControllers[n]` (`[HEADER] NAME` + stages). At least one is required. |
| `ctrl_ers_front_N.ini` | `ersPowerControllersFront[n]` |

`kineticRecovery` is set to 0.5 by `init` (ctor 1.0).

### 4.6 Controller files (`DynamicController::DynamicController` 0x1402af330)

Sections `[CONTROLLER_0]`, `[CONTROLLER_1]`, … (a `do … while (hasSection(next))` loop: it stops
at the first missing index **[code]**). Per stage:
`INPUT` (one of `BRAKE GAS STEER LATG LONG SPEED_KMH GEAR SLIPRATIO_MAX SLIPRATIO_AVG
SLIPANGLE_FRONT_AVG SLIPANGLE_FRONT_MAX SLIPANGLE_REAR_AVG SLIPANGLE_REAR_MAX OVERSTEER_FACTOR
REAR_SPEED_RATIO STEER_DEG CONST RPMS WHEEL_STEER_DEG LOAD_SPREAD_LF LOAD_SPREAD_RF
AVG_TRAVEL_REAR SUS_TRAVEL_LR SUS_TRAVEL_RR`), `COMBINATOR` (`ADD` or `MULT`), `LUT` (file or
inline; not for `CONST`), `CONST_VALUE`, `FILTER` (stored as
`lagToLerpDeltaK(FILTER, 0.004, 0.003)` = `(1 − FILTER) · 444.44`, function at 0x14005d7c0),
`UP_LIMIT`, `DOWN_LIMIT`.

Controller files that feed this system and how many cars ship them: `ctrl_single_lock.ini` 12
(10 loaded, see 4.2), `ctrl_awd2.ini` 8 (of the 9 `AWD2` cars; `ks_alfa_romeo_155_v6` has none),
`ctrl_turbo0.ini` 6, `ctrl_ers_0.ini` 5, `ctrl_turbo1.ini` 2,
`ctrl_awd_center_lock.ini` 1, `controller_kers.ini` 1 (named by `kers.ini`),
`ctrl_awd_front_share.ini` 0, `ctrl_wastegateN.ini` 0. (`ctrl_ebb.ini`, `ctrl_arb_*.ini`,
`ctrl_4ws.ini` belong to brakes, anti-roll bars and steering. `ctrl_diff_power.ini` /
`ctrl_diff_coast.ini` in one mod car are not opened by acs.exe.)

### 4.7 Setup (`SetupManager::initItems` 0x140289570) and .rto files

Setup items write directly into the members **[code]**:
`DIFF_POWER`, `DIFF_COAST` (× 0.01), `DIFF_PRELOAD` → `diffPowerRamp/CoastRamp/PreLoad`;
`FRONT_DIFF_*`, `REAR_DIFF_*`, `CENTER_DIFF_*` (POWER/COAST × 0.01, PRELOAD × 1) → `awdFrontDiff`,
`awdRearDiff`, `awdCenterDiff`;
`AWD_FRONT_TORQUE_DISTRIBUTION` (× 0.01) → `awdFrontShare`.
The three `DIFF_*` items are created only when `Drivetrain::hasDynamicControllers` 0x140266d90
is false; the AWD items are always created, with no test of the traction type. So on an AWD2 car
the `REAR_DIFF_*` setup items write `awdRearDiff`, which `step4WD_new` never reads (its rear
differential is `diffPowerRamp/CoastRamp/PreLoad`, reachable only through `DIFF_*`)
**[binding: code; "no effect": interp]**. Data check: 8 of the 9 `AWD2` cars have
`ctrl_awd2.ini`, so `hasDynamicControllers` is true and they get no `DIFF_*` items at all; none
of those 8 lists a differential section in `setup.ini`. The ninth, `ks_alfa_romeo_155_v6`, offers
`[REAR_DIFF_POWER]` and `[REAR_DIFF_COAST]` in its `setup.ini`, i.e. exactly the two items that
land in the unread `awdRearDiff` (the only readers of `awdRearDiff` by name are `step4WD` and
`Telemetry::step`);
`INTERNAL_GEAR_n` (one item per entry of `gears`; `n` is the `gears[]` index, so
`INTERNAL_GEAR_2` is first gear) → `Drivetrain::setGearRatio(n, value)`; `FINAL_RATIO` → `finalRatio`
(these two kinds of item are the only ones `SetupManager::initItems` 0x140289570 marks
`attached = true` itself, so `SetupManager::step` checks them from the first step);
`ENGINE_LIMITER` (× 0.01) → `Engine::limiterMultiplier`;
`COAST_TORQUE_MULT` (× 0.01) → `Engine::coastTorqueMultiplier`.
The `.rto` files (`final.rto` 43 cars, per-gear files, `ratios.rto`) are "name|ratio" lists
(one per line, e.g. `10//70|7.0000`) the setup screen offers for those items. No function holds
a `.rto` literal: the file name comes from the `RATIOS` key of `setup.ini` (`[GEAR_n]`,
`[FINAL_GEAR_RATIO]`), and the only two functions that use that key are `SetupScreen::loadINI`
0x14017d950 and `AIDriver::AIDriver` 0x140298c60 — not `Drivetrain` **[code: string usage; the
parsing itself was not read]**.

### 4.8 Conventions

- **Gear index** **[code]**: `gears[0]` = reverse (`GEAR_R`, a negative number), `gears[1]` =
  neutral (ratio 0), `gears[2]` = first gear, … So `currentGear` 0 = R, 1 = N, 2 = 1st.
  Controllers see `GEAR = currentGear − 1`.
- **Total ratio**: `ratio = finalRatio · gears[currentGear].ratio` (engine turns per wheel turn).
  Neutral gives `ratio == 0.0`, which the code tests exactly.
- **Speeds** are rad/s in doubles. `rpm = rad/s · 0.15915507 · 60`. The engine gets its rpm as a
  **float** product: `(float)engine.velocity · 0.15915507f · 60f`.
- **`rootVelocity`** is the gearbox input shaft (clutch output) in engine-side rad/s;
  `drive.velocity` is the differential carrier in wheel-side rad/s; the code expects
  `drive.velocity ≈ rootVelocity / ratio` and prints "NOT GOOD, WE ARE STUFFED" if they differ
  by more than 0.5.
- **Signs**: positive wheel speed = rolling forward; positive torque speeds it up. Reverse works
  through the negative ratio. Brake torque arrives already signed against the wheel's rotation
  inside `feedbackTorque`.
- **Clutch control**: `controls.clutch` = 1 means fully **engaged** (pedal up), 0 = disengaged.
- **Tyre order**: `tyres[0]` LF, `[1]` RF, `[2]` LR, `[3]` RR.

---

## 5. Each step: inputs, maths, outputs

`dt` = 0.003 s. Everything in 5.1–5.6 is **[code]** unless marked.

### 5.1 `Drivetrain::step` 0x14026b130

```
outShaft{LF,RF,L,R}.oldVelocity = velocity
locClutch = powf(car.controls.clutch, 1.5)
currentClutchTorque = 0
stepControllers(dt)
RWD or FWD → step2WD;  AWD → step4WD;  AWD_NEW → step4WD_new
```

`Drivetrain::stepControllers` 0x14026b200 (each only if that controller exists):
`awdFrontShare = eval()`;
centre lock: `awdCenterDiff.power = coast = 0`,
`k = clamp((speed_kmh − 5) · 0.05, 0, 1)`, `awdCenterDiff.preload = (eval() − 20) · k + 20`;
single diff lock: `diffPowerRamp = diffCoastRamp = 0`, `diffPreLoad = eval()` (so the controller
output is a locking torque in Nm).

### 5.2 `Drivetrain::step2WD` 0x1402694e0, in order

1. **Finish a pending shift.** If `gearRequest.request` is up or down and
   `timeAccumulator > timeout`: `request = none`, `currentGear = requestedGear`.
   If a request is (still) pending: `timeAccumulator += dt`.
2. `ratio = (double)finalRatio · gears[currentGear].ratio`; `engine.inertia = acEngine.inertia`.
3. **Wheel-side electric torque.** For each `wheelTorqueGenerators[i]` (KERS attached to wheels):
   `t = getOutputTorque()`; `tyreRight.status.feedbackTorque += t · 0.5`; same for `tyreLeft`.
4. **Gear engagement.** If `lastRatio != ratio`: `reallignSpeeds(dt)`; `lastRatio = ratio` (5.3).
5. **Throttle cut and engine.** If `cutOff > 0`: gas = 0 and `cutOff −= dt`, else
   gas = `car.controls.gas`. Call `Engine::step({gas, 0, 0, rpm}, dt)` (5.5).
6. **Clutch state** → `clutchOpenState` (true = slipping/open):
   - `locClutch < 1.0` → open;
   - else if `engine.velocity == 0` → open only if `rootVelocity != 0`;
   - else open if `|rootVelocity / engine.velocity − 1| ≥ 0.1`, otherwise **locked**.
7. **Inertias.** `Ie = engine.inertia`.
   Engine-side total `Ieng = Ie` in neutral, else
   `(outShaftL.inertia + drive.inertia + outShaftR.inertia) / |ratio²| + clutchInertia + Ie`.
   Wheel-side total `Iw = getInertiaFromWheels()`:
   neutral `IL + Idrive + IR`; clutch open `|ratio²| · clutchInertia + Idrive + IL + IR`;
   clutch locked `|ratio²| · (clutchInertia + Ie) + Idrive + IL + IR`.
8. **Integrate.** `T = acEngine.status.outTorque`, `fbSum = (float)(fbRight + fbLeft)` (float add,
   then widened), `a = fbSum / Iw · dt`.

   | State | Engine side | Wheel side |
   |---|---|---|
   | locked, in gear | `d = dt · (T / Ieng)`; `rootVelocity += d`; `accelerateDrivetrainBlock(d / ratio, true)` | `rootVelocity += a · ratio`; `drive.velocity += a`; both out-shafts `+= a` |
   | locked, neutral | `rootVelocity += T / Ie · dt` | `drive.velocity += a`; both out-shafts `+= a` |
   | slipping, in gear | `Tc = (we − wr) / (|we − wr| + 4.0) · locClutch · clutchMaxTorque` with `we = engine.velocity`, `wr = rootVelocity`; `engine.velocity += (T − Tc) / Ie · dt`; `d = dt · Tc / (Ieng − Ie)`; `rootVelocity += d`; `accelerateDrivetrainBlock(d / ratio, true)` | same as locked, in gear |
   | slipping, neutral | `engine.velocity += T / Ie · dt`; `rootVelocity = engine.velocity` (`Tc` is computed but not applied) | same as locked, neutral |

   `currentClutchTorque = (float)(−Tc)` in the slipping states.
   `Drivetrain::accelerateDrivetrainBlock(x, fromEngine)` 0x1402664c0 adds `x` to
   `drive.velocity` and to both out-shafts (2WD, when `diffType < 2`).
   The out-shaft part of the wheel side is also skipped when `diffType ≥ 2` (no such type is
   ever set).
9. **Differential.** Input torque `Tin = locClutch · T`, or `Tc` when the clutch is slipping
   (`Tc ≠ 0`).
   - `diffType == LSD`:
     `lock = Tin > 0 ? |ratio| · diffPowerRamp · Tin : | |ratio| · diffCoastRamp · Tin |`, then
     `lock += diffPreLoad`. With `wL, wR` the out-shaft speeds and `wD = drive.velocity`:
     - if `|wL − wD| ≥ 0.1f` **or** `|fbRight − fbLeft| > lock` (the diff slips):
       `x = dt · (−((wL − wR) / (|wL − wR| + 0.01f)) · lock / outShaftL.inertia) · 0.5`;
       `wL += x; wR −= x`;
       `y = dt · ((fbRight − fbLeft) / outShaftR.inertia) · 0.5`; `wL −= y; wR += y`.
     - else (the diff holds): `wL = wR = wD`.
   - `diffType == Spool`: `wL = wR = wD`.
10. **Both driven wheels flagged locked** (`tyreLeft.status.isLocked && tyreRight.status.isLocked`):
    `brake = Σ over both of (absOverride · inputs.brakeTorque + inputs.handBrakeTorque)`.
    If `| |ratio| · T |` exceeds `brake` (an absolute value: strong engine braking counts as
    well as drive torque), or the car is faster than 1 m/s → clear both flags.
    Otherwise, if the clutch is open → `rootVelocity = drive.velocity = wL = wR = 0`.
11. **Write back** (skipped for AWD2, which does it later): if the clutch is locked
    `engine.velocity = rootVelocity`; `tyreLeft.status.angularVelocity = (float)wL`,
    `tyreRight… = (float)wR`; `Tyre::stepRotationMatrix(dt)` on both.
12. `totalTorque = | |(float)ratio| · (float)T · locClutch − (fbLeft + fbRight) |` (neutral:
    `|(float)T · locClutch|`). Display/telemetry only.
13. **Reaction torque on the chassis.** `G = (float)(locClutch · T · gears[currentGear].ratio)`
    (gearbox ratio only, no final drive).
    - Rear suspension type `Axle`: `body->addLocalTorque((0, 0, G · axleTorqueReaction))`,
      `rigidAxle->addLocalTorque((0, 0, −G · axleTorqueReaction))`; and if
      `torqueModeEx == reactionTorques`: `(−G, 0, 0)` on the rigid axle (or on the body when
      `axleTorqueReaction == 0`).
    - Otherwise, only if `torqueModeEx == reactionTorques` and neither driven wheel is locked:
      for each driven wheel, `body->addTorque(hubMatrixRow0 · (−G) · 0.5)` where the hub matrix
      comes from `suspension->getHubWorldMatrix` (ISuspension +0x08).

    `Car::torqueModeEx` is only ever assigned `original` (twice, in `Car::Car` 0x14026bf00) and
    no function refers to a `TORQUE_MODE_EX` string (the key exists in one mod car, for a
    third-party extension). So in this build both `reactionTorques` branches look unreachable and
    only the `Axle` torque pair (4/113 cars) is live **[interp: from a member-name and string
    search; a write through a raw offset cannot be ruled out]**.

### 5.3 Answer to tyre.md open question 6: how `status.feedbackTorque` is consumed

- `Tyre::step` 0x140283800 computes for **every** wheel, with
  `B = max(inputs.brakeTorque · absOverride, inputs.handBrakeTorque)` and `w = status.angularVelocity`
  **[code]**:
  tyre model version < 10: `feedbackTorque = (loadedRadius · Fx − sign(w) · B) + rollingResistence + inputs.electricTorque`;
  version ≥ 10: `feedbackTorque = rollingResistence − (sign(w) · B + localMX) + inputs.electricTorque`.
  (A `max` of brake and handbrake here, but their **sum** in step 10 of 5.2.)
  For a non-driven wheel it integrates that itself. For a **driven** wheel it does not: it only
  calls `Tyre::updateLockedState` 0x140285070 (which clears `isLocked` on driven wheels) and then
  sets `isLocked = true` if the sign of the wheel speed differs from the sign stored last step
  (`oldAngularVelocity`) while `totalHubVelocity < 1` **[interp: the negative-sign case is not
  fully visible in the pseudo-C]**.
- The drivetrain then uses the two (or four) numbers twice **[code]**:
  1. their **sum** accelerates the whole driven block as one body: `a = Σfb / Iw · dt` is added to
     `drive.velocity`, to every out-shaft, and (times `ratio`) to `rootVelocity`. `Iw` includes
     the gearbox inertia and, when the clutch is locked, the engine inertia reflected through
     `ratio²`. This is how brakes and road drag slow the engine.
  2. their **difference** `fbRight − fbLeft`, divided by one wheel inertia and halved, is added
     to one out-shaft and subtracted from the other, unless the differential is holding.
- The engine torque is never added to `feedbackTorque`; it enters through `rootVelocity` and
  `accelerateDrivetrainBlock`.
- The resulting out-shaft speeds are written to `Tyre::status.angularVelocity` and
  `Tyre::stepRotationMatrix` is called. The tyre uses that speed in its next step.
- So a driven wheel lags the tyre force by one step exactly like a free wheel does: tyre forces
  of step N give the angular speed used for the slip of step N+1.
- KERS on wheels adds its torque to `feedbackTorque` (half per wheel, a quarter per wheel in AWD)
  before step 1. ERS front motors go through `Tyre::inputs.electricTorque` instead.

`Drivetrain::reallignSpeeds` 0x140269100 (gear just engaged; nothing happens for neutral):
`wD = drive.velocity`;
if `locClutch ≤ 0.9`: `rootVelocity = wD · ratio` (the engine is free, the shaft simply follows
the wheels);
else `rootVelocity −= (1 − Ie / getInertiaFromEngine()) · (rootVelocity / ratio − wD) · |ratio|`
(inertia-weighted common speed);
then `accelerateDrivetrainBlock((float)(rootVelocity / ratio − wD), false)`, and
`engine.velocity = rootVelocity` if the clutch is locked.

### 5.4 `Drivetrain::step4WD` 0x14026a220 (AWD) and `step4WD_new` 0x14026ad80 (AWD2)

**AWD**: steps 1–8 as in 2WD with these differences: wheel-side electric torque is added
`· 0.25` to all four tyres; `fbSum = fb[3] + fb[2] + fb[1] + fb[0]`; the inertia sums include
`outShaftLF/RF`; `accelerateDrivetrainBlock(x, true)` gives the front shafts
`awdFrontShare · x · 2` and the rear shafts `(1 − awdFrontShare) · x · 2` (with
`fromEngine = false`, as `reallignSpeeds` calls it, the share is a fixed 0.5); the wheel-side `a` is
added to all four shafts. Unlike 2WD, the differential input torque `Tin` has **no `locClutch`
factor** when the clutch is locked (it is `T` times the share). Then three differentials, each of the form
`x = dt · (−(dv / (|dv| + 0.1f)) · (preload + lock)) / I · 0.5` (note 0.1, not 0.01, and no
"holding" branch):

| Diff | `Tin` | `dv` | Inertia used | Feedback term |
|---|---|---|---|---|
| rear | `(1 − awdFrontShare) · T` (or `· Tc`) | `wL − wR` | `outShaftL.inertia` | `y = dt · (fb[3] − fb[2]) / outShaftR.inertia · 0.5`; `wL −= y; wR += y` |
| front | `awdFrontShare · T` (or `· Tc`) | `wLF − wRF` | `outShaftLF.inertia` | `y = dt · (fb[1] − fb[0]) / outShaftRF.inertia · 0.5`; `wLF −= y; wRF += y` |
| centre | `T` (or `Tc`) | `(wL + wR)/2 − (wLF + wRF)/2` | `invI = 1 / (IR + IL + ILF + IR)` (no `· 0.5`; `IR` appears twice, `IRF` not at all); `x` is added to both rear shafts and subtracted from both front shafts | `y = dt · ((fb[3] + fb[2]) − (fb[1] + fb[0])) · invI`; rear `+= y`, front `−= y` |

`lock = Tin > 0 ? power · Tin · |ratio| : |coast · Tin · |ratio||`. At the end all four
`status.angularVelocity` are written, `stepRotationMatrix` is called and `status.isLocked` is
cleared on all four. There is no locked-wheel handling and no chassis reaction torque in this path.

**AWD2**: `step2WD(dt)` runs first on the rear axle (`tyreLeft/Right` stay the rear tyres), then:

```
t = ((wL + wR)/2 − (wLF + wRF)/2) · awd2.ramp
if controllers.awd2:  awd2.maxTorque = eval()
t = clamp(t, −awd2.maxTorque, +awd2.maxTorque);   awd2.currentLockTorque = (float)t
k = 1 / (outShaftRF.inertia + outShaftLF.inertia)
wRF += k·t·dt;  wLF += k·t·dt
e0 = fb[0]·k·dt:  wRF = (wRF + e0) − e0;  wLF = wLF + e0 + e0
e1 = fb[1]·k·dt:  wLF = (wLF + e1) − e1;  wRF = e1 + wRF + e1
d = t / getInertiaFromWheels() · dt
if ratio != 0: rootVelocity += ratio · (−d)
accelerateDrivetrainBlock(−d, false)
engine.velocity = rootVelocity if the clutch is locked
write all four angular velocities, stepRotationMatrix, clear isLocked
```

So the front wheels are two independent free wheels plus half of the coupling torque each, and
the rear block loses the same torque **[interp]**. The rear axle keeps everything `step2WD`
does: LSD, locked-wheel logic and chassis reaction torques. The `(x + e) − e` forms are kept as written
because they matter for bit-exact rounding.

### 5.5 `Engine::step` 0x1402880e0, in order

Inputs: `gasInput`, `rpm` (may be below idle or negative). `carSpeed` and `altitude` are always 0.

1. `gas = getThrottleResponseGas(gasInput, rpm)`:
   no curves → unchanged; only `throttle.lut` → `clamp(lut(gas·100) · 0.01, 0, 1)`;
   with `[THROTTLE_RESPONSE]` too → blend of the two maps by `clamp(rpm / RPM_REFERENCE, 0, 1)`.
2. Push-to-pass (`Engine::stepP2P`), only if enabled: on `controls.kers` after the cooldown and
   with activations left, every turbo's `wastegate = baseWastegate + overboost` for `timeS`.
3. Engine-brake setting: if `gasCoastOffset > 0`:
   `s = clamp((rpm − minimum) / coastEntryRpm, 0, 1) · gasCoastOffset`;
   `gas = clamp((1 − s) · gas + s, 0, 1)` (a little throttle is held open on the overrun).
4. **Limiter**: if `limiter != 0` and `rpm > limiter · limiterMultiplier` → `limiterOn = limiterCycles`.
   While `limiterOn > 0`: `gas = 0`, `limiterOn −= 1`.
5. If `lifeLeft ≤ 0` → `fuelPressure = 0`.
6. `gas *= electronicOverride` (traction control / pit limiter cut); `gasUsage = gas`.
7. `Tp = powerCurve(rpm)`; `coast = coast1 != 0 ? (rpm − minimum) · coast1 : 0`.
8. `stepTurbos()` (5.6); if `status.turboBoost != 0`: `Tp *= 1 + turboBoost`.
9. If `coast2 != 0`: `coast −= (rpm − minimum)² · coast2 · sign(rpm)`.
10. If there are coast generators (ERS): `externalCoastTorque = Σ getCoastTorque()`;
    `coast += externalCoastTorque`.
11. If `rpm ≤ minimum`: `coast = 0`, `externalCoastTorque = 0`.
12. `bov = ((1 − gas) · turboBoost > bovThreshold) ? 1 : 0`.
13. **Damage**: if `turboBoostDamageThreshold != 0` and boost above it:
    `lifeLeft −= (boost − thr) · turboBoostDamageK · 0.003 · mechanicalDamageRate`;
    if `rpmDamageThreshold != 0` and rpm above it:
    `lifeLeft −= (rpm − thr) · rpmDamageK · 0.003 · mechanicalDamageRate`.
    `lifeLeft` starts at 1000 (`Engine::reset`); `Engine::blowUp` sets −100.
14. **Air**: `air = getAirDensity() · 0.82630974` with
    `getAirDensity = 1.2922 − ambientTemperature · 0.0041` (so `air` ≈ 1.0 at 20 °C ambient);
    if `restrictor > 0`: `air = max(0, air − restrictor · rpm · 0.0001 · gas)`.
15. `outTorque = ((Tp − coast) · gas + coast) · air`
    (full throttle → boosted curve torque; closed throttle → the negative coast torque).
16. If `fuelPressure > 0`:
    - `rpm ≥ minimum` and `overlapGain != 0`:
      `outTorque += (sin(physicsTime · 0.001 · overlapFreq · rpm · 0.000333…) · 0.5 − 0.5) · |rpm − overlapIdealRPM| · overlapGain`;
    - `rpm < minimum`, stall disabled (the normal case): `outTorque = max(15.0, outTorque)` —
      **this floor of 15 Nm is the whole idle controller**;
    - `rpm < minimum`, stall enabled: `outTorque = rpm · −0.01`, or `starterTorque` (20) while
      `GetAsyncKeyState(8)` (Backspace) is down.
17. If `fuelPressure < 1`: `outTorque = (outTorque − drag) · fuelPressure + drag` with
    `drag = rpm · −0.01` (a dead engine only drags).
18. `outTorque += Σ torqueGenerators[i]->getOutputTorque()` (ERS, KERS on engine).
19. `status.isLimiterOn = limiterOn != 0`; **`electronicOverride = 1.0`** (re-armed every step);
    `maxPowerW_Dynamic = max(…, rpm · outTorque · 0.1047)`.

Not in the step: `coastTorqueMultiplier` (the `COAST_TORQUE_MULT` setup item) is only used by
`Engine::getTorqueAtRPM` (AI/engineer estimates), not by `Engine::step`. `data.coast0`,
`data.coastCurve`, altitude and car speed are unused.

### 5.6 Turbo (`Engine::stepTurbos` 0x140288900, `Turbo::step` 0x1402ae7c0)

```
for each turbo controller:  target turbo.maxBoost (or .wastegate if isWastegate) = controller.eval()
status.turboBoost = 0
for each turbo:
    rpm' = lastInput.rpm > 0 ? lastInput.rpm : 0
    target = (rpm' > 0 and gas > 0) ? powf(clamp(gas · rpm' / rpmRef, 0, 1), gamma) : 0
    k = clamp(0.003 · (target > rotation ? lagUP : lagDN), 0, 1)       // 0.003 is a literal, not dt
    rotation += (target − rotation) · k
    if wastegate != 0 and wastegate · userSetting < maxBoost · rotation:
        rotation = wastegate · userSetting / maxBoost
    status.turboBoost += maxBoost · rotation · fuelPressure
```

`gas` here is the value after steps 1–6 of 5.5, so a limiter or traction-control cut also spools
the turbo down. `userSetting` is the cockpit boost level (`Engine::setTurboBoostLevel`), 1.0 for
non-adjustable turbos.

### 5.7 Gear changes

`GearChanger::step` 0x1402bab50: if `controls.requestedGearIndex == −1` (paddles): a rising edge
of `controls.gearUp` calls `Drivetrain::gearUp`, a rising edge of `controls.gearDn` calls
`Drivetrain::gearDown`. Otherwise (H-shifter): `Drivetrain::setCurrentGear(requestedGearIndex, false)`.

`Drivetrain::gearUp` 0x1402669f0: refused if the gearbox is locked, if already in top gear, or
if a request is pending. Else `gearRequest = {up, timeout = gearUpTime, requestedGear = current + 1}`,
fire `evOnGearRequest`, `cutOff = autoCutOffTime` (if non-zero), and **`currentGear = 1`
(neutral) for the duration of the shift**.

`Drivetrain::gearDown` 0x140266660: same with `gearDnTime` and `current − 1`, no throttle cut,
after the downshift protection. Both checks below are skipped when
`downshiftProtection.isActive` is false or `currentGear < 1` (so `lockN` does nothing on a car
whose protection is inactive, e.g. the default for H-shifter cars):
- both current and target ratios non-zero:
  `decL = min(0, (wL − wL_old) · 333.33333043588533)` (a double constant, `1 / (double)0.003f`;
  `wL_old` is `outShaftL.oldVelocity`, saved at the top of the last `Drivetrain::step`), same for R;
  `projected rpm = targetRatio · ((gearDnTime·decR + wR + gearDnTime·decL + wL) · 0.5) · finalRatio · 0.15915507 · 60`;
  if `projected > getLimiterRPM() + overrev` → refuse (and call `downshiftProtectionFunction`,
  a UI callback installed by `CarAvatar::initPhysics`).
- otherwise: `lockN` and the target is neutral (ratio 0) and speed > 2 km/h → refuse.

`Drivetrain::setCurrentGear(i, force)` 0x1402692e0 (H-shifter): gearbox locked and `i != 1` →
neutral. Otherwise `isGearGrinding = false`, and nothing more happens unless `i` is a valid
index different from `currentGear`. For a real gear (`i != 1`) and not forced:
`diff = |engine.velocity − gears[i].ratio · drive.velocity · finalRatio|`,
`a = locClutch · diff`,
`mismatch_rpm = ((gas · locClutch · diff − a) · controlsWindowGain + a) · 0.15915507 · 60`
(doubles; the triple product is evaluated left to right, it is not `gas · a`);
if `mismatch_rpm ≥ validShiftRPMWindow` → `isGearGrinding = true`, the gear does **not** engage,
and (damage, only while `validShiftRPMWindow > 0` and `mechanicalDamageRate > 0`)
`validShiftRPMWindow −= damageRpmWindow · 0.003 · mechanicalDamageRate`.
Else `currentGear = i` immediately (no shift time, no cut). A fully pressed clutch gives `a = 0`
and engages, unless wear has already brought `validShiftRPMWindow` down to 0 or below.

`Drivetrain::isGearboxLocked` 0x1402673c0: after the session start
(`physicsTime > startTimeMS`) it returns `car.isGearboxLocked`. Before the start the gearbox is
locked, except during the last `lockGearboxAtStartTimeMS` before the start
(`physicsTime ≥ startTimeMS − lockGearboxAtStartTimeMS`) when the jump-start mode is not
"lock on grid".

### 5.8 Clutch, blip and shift assists

`Autoclutch::step` 0x1402b9590 (called from `Car::step`, before the components):
```
if a sequence is playing:
    if speed > 5 km/h: clutchValueSignal = curve(time); time += dt; controls.clutch = clamp(signal); return
    else: stop the sequence
if not useAutoOnStart and not isForced: return            // driver's own clutch stays
rpm, gear = drivetrain.currentGear;  target = 1
gear is R or 1st:  rpmMin ≤ rpm ≤ rpmMax → target = signal = (rpm − rpmMin)/(rpmMax − rpmMin)
                   rpm < rpmMin → target = signal = 0
neutral:           speed < 5 km/h → (gas > 0.2 ? signal = 1 : target = signal = 0)
higher gears:      rpm < rpmMin → target = signal = 0
signal moves toward target by at most dt · clutchSpeed (clutchSpeed = 1.0)
controls.clutch = clamp(signal, 0, 1)
```
`Autoclutch::onGearRequest` 0x1402b9350: if `useAutoOnChange` and the clutch signal is above
0.01, start the up- or down-shift profile (only if it has 4 points).

`AutoBlip`: the gear-request handler (0x1402b9880) stores `blipStartTime = physicsTime` on a
downshift request when `controls.clutch > 0.1`. `AutoBlip::step` 0x1402b9ef0: unless controls
are locked or speed < 5 km/h, and if `isElectronic || isActive`, for
`0 ≤ t = physicsTime − blipStartTime < blipPerformTime` (ms), and only if `blipProfile` has its
4 points:
`controls.gas = clamp(max(controls.gas, blipProfile(t)), 0, 1)`.

`AutoShifter::step` 0x1402ba7f0: nothing before 300 ms into the session, when inactive, or in
reverse. First use with `changeUpRpm == 0`: `changeUpRpm = min(limiter, maxPowerRPM) · 0.98`,
`changeDnRpm = maxTorqueRPM · 1.1`. Nothing if the driver is already pressing a shift control.
With `spin = drivenTyresSlip > slipThreshold and speed > 5 m/s`: if not shifting, clutch > 0.99
(or neutral) and no spin: `rpm > changeUpRpm`, not top gear, gas > 0.2, no cutoff running →
`controls.gearUp = true`, `gasCutoff = gasCutoffTime`; `rpm < changeDnRpm` (× 0.65 in 2nd gear),
gear above 1st, clutch > 0.85, no cutoff running → `controls.gearDn = true`. Also downshift when
nearly stopped (speed < 2 m/s, gas < 0.1, gear above 1st, not shifting, no cutoff running). While `gasCutoff > 0`: `gasCutoff −= dt`, `controls.gas = 0`.

### 5.9 Fuel, water, damage hooks (in `Car`)

- Fuel burn, `Car::step` **[code]**:
  `fuel −= |rpm| · dt · gasUsage · (max(0, turboBoost) + 1) · fuelConsumptionK · 0.001 · fuelConsumptionRate`.
  `fuelConsumptionK` is `car.ini [FUEL] CONSUMPTION` (`Car::initCarData` 0x140272b30);
  `fuelConsumptionRate` is the session multiplier in `PhysicsEngine`. Then
  `Engine::fuelPressure = fuel > 0 ? 1 : 0`. `fuel_cons.ini` is opened in `Car::Car` only for
  `[FUEL_EVAL] KM_PER_LITER` → `Car::expectedFuelPerLap` (the fuel-per-lap estimate); it does
  not enter the burn **[code]**.
- Water temperature, `Car::stepThermalObjects` 0x1402769f0: if `rpm > 0.8 · minimum`:
  `water.addHeadSource((rpm / limiterRPM) · 20 · controls.gas + 85)` (adds to `heatAccumulator`);
  then `ThermalObject::step(dt, ambientTemperature, speed)` 0x1402b2f00 **[code]**:
  `t += ((1 − coolSpeedK · speed_m/s) · ambient − t) · (1 / tmass) · dt · coolFactor`; and if the
  accumulated source `h != 0`: `t += (h − t) · (1 / tmass) · dt · heatFactor`; `heatAccumulator = 0`.
  Values: `tmass` 20 and `coolSpeedK` 0.002 (`Car::initCarData`), `coolFactor` 0.2 and
  `heatFactor` 1.0 (constructor 0x1402b2e80); `Car::reset` sets `water.t = 60`.
  Nothing reads it back into the engine.
- There is no separate "fuel pressure" model: `fuelPressure` is only 0 or 1.

### 5.10 `Kers::step` 0x1402b7e10 and `Kers::getOutputTorque` 0x1402b72b0

```
if car is in pits: charge = 1
w = (float)(|outShaftL.velocity| · 2);  if w < 6: w = 0
brake = Σ over 4 tyres of absOverride · inputs.brakeTorque
charge = clamp(charge + brake / brakeForMaxCharge · w · 0.5 · chargeK · dt, 0, 1)
input = hasController ? (controls.brake > 0.01 ? 0 : clamp(controller.eval(), 0, 1))
                      : (controls.kers ? 1 : 0)
if hasButtonOverride and controls.kers: input = 1
if charge ≤ 0 or speedLimiter.isLimiting or (maxJ != 0 and currentJ > maxJ): input = 0
charge = clamp(charge − input · dischargeK · dt, 0, 1)
if engineRPM ≥ limiterRPM or currentGear < 2 (R or N): input = 0
if input > 0: w2 = engine.velocity (ENGINE) or (|wR| + |wL|)/2 (WHEELS)
              currentJ += torqueLUT(w2 as rpm) · input · w2 · dt
```
Torque: `present and charge > 0 ? torqueLUT(rpm of the attachment) · input : 0`.
`currentJ` is zeroed on every completed lap (handler 0x1402b7270 on `Car::evOnLapCompleted`).
`Kers::reset` 0x1402b7e00: `charge = 1`, `currentJ = 0` (callers in 3.2).

### 5.11 `ERS::step` 0x1402930e0

```
if car is in pits: charge = 1
coastT = max(0, −(outTorque − externalCoastTorque))               // values of the previous Engine::step
brakeT = ratio == 0 ? 0 : (|tyres[2].brakeTorque|·abs[2] + |tyres[1].brakeTorque|·abs[1]) / ratio
if ratio == 0: coastT = 0
heatToBattery = isHeatCharginBattery and not (hasButtonOverride and controls.kers)
boostFrac = Σ maxBoost > 0 ? status.turboBoost / Σ maxBoost : 0
if charge < 1:
    status.kineticRecovery = (brakeT + coastT) · chargeK · rootVelocity · kineticRecovery
    status.heatRecovery    = heatToBattery ? boostFrac · heatChargeK : 0
    charge = clamp(charge + max(0, kin + heat) · dt, 0, 1)
else both recoveries = 0
isCharging = status.kineticRecovery > 0
input      = clamp(controller.eval(), 0, 1);  inputF = clamp(controllerFront.eval(), 0, 1)
heatAssist = heatToBattery ? 0 : heatTorque · controls.gas · boostFrac
if hasButtonOverride and controls.kers: input = inputF = 1; heatAssist = boostFrac · heatTorque
if charge ≤ 0 or speedLimiter.isLimiting or currentJ > maxJ: input = inputF = 0
if engineRPM ≥ limiterRPM or currentGear < 2: input = 0
if input ≥ 0:
    need = (clamp(input − heatAssist, 0, 1) · dischargeK + inputF · dischargeKFront) · dt
    need ≤ charge ? charge −= need : (input *= charge/need; inputF *= charge/need; charge = 0)
input = clamp(heatAssist + input, 0, 1)
if input > 0: currentJ += torqueLUT(engine rpm) · clamp(input − heatAssist, 0, 1) · engine.velocity · dt
if ratio != 0: car.brakeSystem.rearCorrectionTorque = rearCorrectionTorque · kineticRecovery · ratio
if front motors: bias = clamp((load[0]/(load[0]+load[1]) − 0.5) · frontTorqueVectoringBias + 0.5, 0, 1)
                 Tf = frontTorqueLUT(avg front wheel rpm) · inputF;  currentJ += Tf · wFrontAvg · dt
                 tyres[0].inputs.electricTorque = Tf · bias;  tyres[1]… = Tf · (1 − bias)
```

- Drive torque (`ERS::getOutputTorque`): `torqueLUT(engine rpm) · input`, added to the **engine**
  torque (step 18 of 5.5). Front motors act on the **front wheels** through the tyre input.
- Regeneration drag (`ERS::getCoastTorque`): if `charge < 1` and in gear:
  `−coastLUT(engine rpm) · controls.clutch · kineticRecovery`, added to the engine coast torque
  (step 10 of 5.5). Member offsets here were resolved by hand because the function runs on the
  second base class (`this + 8`).
- MGU-H: `isHeatCharginBattery` true → turbo boost charges the battery (`heatChargeK`); false →
  the same boost fraction becomes extra assist (`heatTorque`) that does not drain the battery.
- Energy budget: `currentJ` is zeroed on every completed lap (handler 0x140291980 on
  `Car::evOnLapCompleted`) and by `ERS::reset` 0x140292fa0 (`charge = 1`, `currentJ = 0`; callers
  in 3.2). Unlike KERS the `currentJ > maxJ` test has no `maxJ != 0` guard, so with
  `MAX_KJ_PER_LAP` missing or 0 delivery stops as soon as any energy has been counted **[code]**.
- Front motors: if `load[0] + load[1] ≤ 0` both `electricTorque` values are set to 0.
- Cockpit controls: `kineticRecovery` (MGU-K recovery level), the active power controller
  (`ERS::setPowerController`, MGU-K delivery profile), `isHeatCharginBattery` (MGU-H mode) and the
  engine-brake level (`Engine::setCoastSettings`) are changed from the game side
  (`CarAvatar::cycleERSRecovery` 0x1400d2bc0, `cycleERSPower` 0x1400d2a80,
  `cycleERSHeatCharging` 0x1400d29e0) **[interp]** — the bodies of those were not read.

### 5.12 `DynamicController::eval` 0x1402b0c00

```
out = 0
for each stage:
    v = (input == CONST) ? constValue : lut(getInput(input))
    if |v − currentValue| ≥ 0.001: v = (v − currentValue) · clamp(filter · 0.003, 0, 1) + currentValue
    currentValue = v
    ADD: out += v;   MULT: out *= v;   undefined: out = 0
    if upLimit != 0 or downLimit != 0: out = clamp(out, downLimit, upLimit)
return out
```
With `filter = (1 − FILTER) · 444.44` the per-step blend factor is `(1 − FILTER) · 1.3333`
(the same expression as the turbo lag). Inputs (`DynamicController::getInput` 0x1402b0d70) are
read live from the car: pedals, `accG.x/z`, speed · 3.6, `currentGear − 1`, driven-tyre slip
ratios (signed max / mean, for RWD, FWD and AWD only: on an `AWD2` car `SLIPRATIO_MAX` and
`SLIPRATIO_AVG` return 0), slip angles in degrees (`*_AVG` is the signed mean, `*_MAX` the larger
absolute value), oversteer factor
(`DynamicController::getOversteerFactor` 0x1402b11d0), rear/front wheel speed ratio
(`DynamicController::getRearSpeedRatio` 0x1402b1230), `steerLock · steer`, engine rpm,
`finalSteerAngleSignal`, front load split, rear suspension travel in mm.

---

## 6. Connections to other systems

Position in `Car::stepComponents` matters because several links are one step late.

### Inputs (who writes what this system reads)

| Input | Written by | When |
|---|---|---|
| `Car::controls.gas/clutch/gearUp/gearDn/requestedGearIndex/kers` | `Car::pollControls` 0x140274e70 from the controls provider (wheel / keyboard / pad, AI, drag AI; there is no replay or network provider: replayed and remote cars never run `Car::step`, see `docs/map/car_step.md` section 2), in `Car::step` | before everything |
| `controls.gas/brake/steer/clutch` | forced by `Car::step` right after `pollControls` **[code]**: controls locked (`isControlsLocked` or `physicsTime < lockControlsTime`) → gas 0, brake 1, steer 0, clutch 0; `isGentleStopping` → gas 0, brake 0.2 | `Car::step`, before `Autoclutch::step` |
| `controls.clutch` | overwritten by `Autoclutch::step` | `Car::step`, before `stepComponents` |
| `controls.gas` | raised by `AutoBlip::step`, zeroed by `AutoShifter::step` | just before `Drivetrain::step` |
| `Tyre::status.feedbackTorque`, `status.isLocked` (driven wheels) | `Tyre::step` | earlier in the same step |
| `Tyre::inputs.brakeTorque`, `absOverride`, `handBrakeTorque` | `BrakeSystem::step` (first component) writes the brake torques **[code]**; `absOverride` of all four tyres is written by `ABS::step` 0x14028f610, which runs after the drivetrain, so the tyre and the drivetrain see the value of the previous step **[code]**. One more writer exists: `AIDriver::stepSuperhuman` 0x1402a2a60 sets each tyre's `absOverride` to 0 or 1 (AI cars; not read further here) | |
| `Tyre::data.angularInertia` | tyres.ini through `Tyre::setCompound` 0x1402834e0; copied into the out-shaft inertias once in `Drivetrain::loadINI`. On `AWD` the loader also **writes** the average `(tyre[0] + tyre[2]) / 2` back into all four tyres (4.2). `setCompound` rewrites the tyre member on every compound change; the out-shaft copies are not refreshed | load |
| `Engine::electronicOverride` | `TractionControl::step`, `SpeedLimiter::step` set 0 | **after** `Drivetrain::step`; consumed by the next `Engine::step`, which resets it to 1 |
| `Engine::fuelPressure` | `Car::step` (fuel), `Engine::step` (damage) | |
| `Engine::restrictor` | `Car::setRestrictor` 0x140275d10 (multiplayer ballast/restrictor) | event |
| `Engine::limiterMultiplier`, `coastTorqueMultiplier`, diff and gear members | setup items | setup change |
| `Turbo::userSetting` | `Engine::setTurboBoostLevel` from `CarAvatar::setTurboBoost` 0x1400da9e0 and `AIDriver::stepTurbo` 0x1402a30c0 | event |
| `PhysicsEngine::ambientTemperature`, `mechanicalDamageRate`, `physicsTime`, `fuelConsumptionRate` | session | |
| `Car::isGearboxLocked`, session start time | race rules (`Drivetrain::isGearboxLocked`) | |
| `Car::axleTorqueReaction`, `suspensionTypeR` | suspension loading in `Car::Car`: `suspensions.ini [AXLE] TORQUE_REACTION`, read only when the rear type is `AXLE` (ctor default 1.0; 4/113 cars, all negative values) | load |
| `Car::torqueModeEx` | `Car::Car` sets `original`; no other writer found (5.2 step 13) | load |
| `SpeedLimiter::isLimiting` | `SpeedLimiter::step` (previous step) | read by KERS/ERS |

### Outputs (who reads what this system writes)

| Output | Read by |
|---|---|
| `Tyre::status.angularVelocity` of driven wheels, `Tyre::stepRotationMatrix` | `Tyre::step` next step (slip ratio), graphics |
| `Tyre::status.isLocked` (cleared) | `Tyre::step` |
| Chassis / axle reaction torques (`IRigidBody::addLocalTorque` +0xe0, `addTorque` +0x118) | ODE step |
| `Tyre::inputs.electricTorque` (front tyres, ERS front motors) | `Tyre::step` next step |
| `BrakeSystem::rearCorrectionTorque` (ERS) | `BrakeSystem::step` next step: subtracted from the rear brake torque |
| `Engine::gasUsage`, `status.turboBoost` | `Car::step` fuel burn |
| `drivetrain.engine.velocity` (rpm), `currentGear`, `status.outTorque`, `status.turboBoost`, `bov`, `totalTorque`, `currentClutchTorque`, `isGearGrinding`, KERS/ERS charge and input | `Car::getPhysicsState` 0x140270d70 → audio, HUD, replay, shared memory; AI; `Telemetry::step` |
| `Drivetrain::getEngineRPM` 0x140266b80, `Engine::getLimiterRPM` 0x140285a50 | autoshifter, KERS/ERS, controllers, water temperature, fuel burn in `Car::step`, force-feedback rumble in `Car::pollControls`, telemetry. **Not** the autoclutch: `Autoclutch::step` reads `drivetrain.engine.velocity` itself and converts it in double precision (see the rpm trap in section 7) |
| `gearRequest`, `cutOff` | `AIDriver::stepGears` |
| `evOnGearRequest` | `Autoclutch`, `AutoBlip` |
| `Car::evOnPush2Pass` | game layer (HUD) |

---

## 7. Port notes

**Effort: M** (L if KERS/ERS and all controller inputs are wanted in the first pass).
The core that every car needs — `Engine::step`, `Turbo::step`, `Drivetrain::step`/`step2WD` and
helpers, gear changes — is about 600 lines of straightforward arithmetic with no dependence on
the rigid-body library except three "add torque" calls. The bulk of the remaining code is ini
loading.

Suggested split:
1. **S** — `Curve` look-up (already needed by the tyre port; check the same interpolation is used).
2. **S** — `Engine` + `Turbo` (no controllers): pure function of `(gas, rpm, state)`.
3. **M** — `Drivetrain` 2WD: clutch, gear timing, LSD/spool, locked-wheel logic, `setCurrentGear`.
4. **S** — `GearChanger`, `Autoclutch`, `AutoBlip`, `AutoShifter` (the last needs
   `RaceEngineer::getDrivingTyresSlip`).
5. **S** — `DynamicController` (needs live car values; suspension travel inputs need the
   suspension port).
6. **S/M** — `step4WD`, `step4WD_new` (14 cars), `Kers` (2 cars), `ERS` (5 cars).

Needed before it: the tyre (done), `Curve`/`INIReader` equivalents, a `Car` shell holding
`controls`, `tyres[4]`, speed, `accG`; a rigid-body handle only for the reaction torques (can be
stubbed while `torqueModeEx == original` and the rear suspension is not an axle).
For the F2004 target car (`cardata/ks_ferrari_f2004`: RWD, LSD 0.20 / 0.30 / 20 Nm, no turbo,
no kers.ini / ers.ini, no drivetrain controller files, downshift autoclutch profile only)
steps 1–4 are enough.

**Bit-exact traps**
- Mixed precision: speeds and inertias are doubles, torques from tyres and most parameters are
  floats. Keep each conversion where the original has it (e.g. `fbRight + fbLeft` is a float add;
  rpm is `(float)w · 0.15915507f · 60f`; `locClutch` is a float from `powf`).
- Two flavours of rad/s → rpm **[code]**. Float: `(float)w · 0.15915507f · 60f` in
  `Drivetrain::getEngineRPM`, the `rpm` handed to `Engine::step`, and the LUT look-ups inside
  `Kers::step` / `ERS::step`. Double, narrowed at the end: `(float)(w · 0.15915507152579872 · 60.0)`
  in `Autoclutch::step`, `Kers::getOutputTorque`, `ERS::getOutputTorque`, `ERS::getCoastTorque`;
  the same double constants in `setCurrentGear` and `gearDown` (compared as doubles) and in
  `projectRPMAtDownshift` (narrowed on return). The two can differ in the last bit, so each call
  site must keep its own form (checked in the disassembly of `getEngineRPM`, `Autoclutch::step`
  and `ERS::getCoastTorque`; the others from the pseudo-C).
- Float constants widened to double: `0.1f`, `0.01f` in the differential, but a true double `0.1`
  in the clutch-lock test and `4.0` in the clutch torque.
- Order of additions in the inertia sums differs between the two helpers; copy it from the
  pseudo-C.
- `powf`, `sin` come from MSVCR120; the tyre port already had to match that library.
- The limiter counts steps, the turbo uses a literal 0.003, damage uses a literal 0.003.
- `Engine::step` reads the keyboard (`GetAsyncKeyState`) only in the stall branch, which is off
  by default.

**Oracle test (same idea as the tyre oracle)**
- `Turbo::step` and `Engine::step` can be called directly on a constructed `Engine`:
  fake a `PhysicsEngine` (only `ambientTemperature`, `mechanicalDamageRate`, `physicsTime` are
  read), a `Car` pointer (only `ksPhysics`, `controls.kers`, `evOnPush2Pass` are reached), fill
  `data.powerCurve` through `Engine::loadINI` on a real car folder or by hand. Sweep
  `(gas, rpm)` sequences and compare `status.outTorque`, `status.turboBoost`, `limiterOn`,
  `lifeLeft`, turbo `rotation` bit for bit.
- `Drivetrain::step` needs a `Car` with `controls`, `tyres[4]` (only `status.feedbackTorque`,
  `status.isLocked`, `status.angularVelocity`, `inputs`, `absOverride`, `data.angularInertia`,
  plus whatever `Tyre::stepRotationMatrix` touches), `ksPhysics`, `getSpeed()`, and a body whose
  `addLocalTorque`/`addTorque` can be recorded. The cheapest route is the one planned for the
  whole-car oracle: construct a real `Car` in-process from a car folder, then drive
  `Drivetrain::step` with scripted `controls` and scripted `feedbackTorque` per wheel, recording
  the six `GearElement`s, `rootVelocity`, `clutchOpenState`, `currentGear`, `gearRequest` and
  `cutOff` after each step.
- Gear logic: call `Drivetrain::gearUp/gearDown/setCurrentGear` between steps with scripted wheel
  speeds; `isGearboxLocked` needs `PhysicsEngine::hasSessionStarted` to return true (fake session
  times).
- `DynamicController::eval`, `Kers::step`, `ERS::step`: callable on their own with the same fake
  `Car`; they read `Car::isInPits`, `speedLimiter.isLimiting` and the drivetrain members.

---

## 8. Open questions

1. **`Curve::getValue` interpolation** (linear vs spline, clamping outside the table) decides the
   exact torque from `power.lut`, `throttle.lut` and every controller LUT. Not read here; the
   tyre port may already have settled it.
2. *(Closed by the review.)* `[KINETIC] BRAKE_REAR_CORRECTION`, `[COCKPIT_CONTROLS] RECOVERY` and
   `[FRONT_MOTORS] FRONT_TORQUE_VECTORING_BIAS`: the section literals are visible next to each
   key in the pseudo-C of `ERS::init`, so these three mappings are **[code]**.
3. **ERS kinetic recovery reads brake torque from `tyres[2]` and `tyres[1]`** (left rear and
   right front). Confirmed in the disassembly (Car+0x1bd8 and Car+0x1380). It looks like a slip in
   the original source (both rears were probably meant); a bit-exact port has to copy it.
4. **Centre differential inertia in `step4WD`** uses `outShaftR.inertia` twice and never
   `outShaftRF.inertia`. Harmless because the AWD loader makes all four equal, but copy it as is.
5. *(Closed by the second review.)* **`step4WD_new` front-wheel update**: the disassembly at
   0x14026ad80 has the same order as 5.4: `k = 1 / (IRF + ILF)`; `(k · t) · dt` added to both;
   for `fb[0]`: LF `+= e0`, RF `+= e0`, RF `−= e0`, LF `+= e0`; for `fb[1]`: `(e1 + LF) − e1`,
   `(e1 + RF) + e1`. The matching check for `step4WD` was not done.
6. **`[AWD2] FRONT_DIFF_*`** is loaded into `awdFrontDiff` but I found no use in
   `step4WD_new`; the front axle behaves as open. Setup items `FRONT_DIFF_*` can still change the
   values.
7. **`EDL::step`** 0x1402bb460 (runs before the tyres): resolved in `docs/map/electronics.md`
   4.1 / 5.3. It reads electronics.ini `[EDL]` and adds a brake torque to the faster wheel of the
   driven pair through `Tyre::inputs.brakeTorque`, so its effect reaches the drivetrain inside
   `feedbackTorque`.
8. **Who toggles `Engine::isEngineStallEnabled`, `Autoclutch::isForced` at run time,
   `AutoBlip::isActive`, `AutoShifter::isActive`** (driving-assist settings): seen for
   `AIDriver::AIDriver`, `ACPlugin::setAutoShift` and `CarAvatar::setAutoClutchEnabled`; the
   full list of writers was not traced. No writer of `isEngineStallEnabled` other than
   `Engine::init` (false) was found by name search, so the stall/starter branch is probably dead.
   (`PhysicsEngine` has a member of the same name, set in its constructor; nothing was found
   copying it into `Engine`.) Seen in the review: `Car::Car` sets `autoBlip.isActive = false`
   but `AutoBlip::init`, which runs later, sets it to true; `AutoShifter::init` leaves
   `isActive = false`; `Autoclutch::init` sets `useAutoOnStart = useAutoOnChange = true`.
9. **Cockpit ERS / engine-brake / turbo controls** (`controls.turboUp`, `engineBrakeUp`,
   `MGUKDeliveryUp`, …): the physics classes read none of these flags. They are consumed on the
   game side (`CarAvatar`, notifier classes) which then calls `setTurboBoostLevel`,
   `setCoastSettings`, `setPowerController` or writes `kineticRecovery` /
   `isHeatCharginBattery`. The exact value steps were not read.
10. *(Closed by the review.)* `fuel_cons.ini`: `Car::Car` reads only `[FUEL_EVAL] KM_PER_LITER`
    into `Car::expectedFuelPerLap`. The per-step burn formula in 5.9 is from the code.
11. **`.rto` files**: reached only through the `RATIOS` key of `setup.ini`, used by
    `SetupScreen::loadINI` 0x14017d950 and `AIDriver::AIDriver` (string usage); never by the
    drivetrain. The line format `name|ratio` is from the data files; the parser was not read.
12. **`Drivetrain::lockCounter[4]`** is zeroed in `init` and not used by any function read here.
13. **`Kers` negative input**: the `negativeInputChargeK` branch needs `input < 0`, which the
    clamps before it make impossible; treated as dead code.
14. **`ERS::init` first builds `controller` from the `ers.ini` path itself** (no
    `[CONTROLLER_n]` sections there, so it is empty) before `setPowerController` replaces it.
    Read as written; harmless.
15. **AutoShifter slip test** depends on `RaceEngineer::getDrivingTyresSlip`, not read here.
16. **Autoclutch nesting**: the three gear cases in 5.8 were rebuilt from goto-heavy pseudo-C;
    the logic was cross-checked once but not against the disassembly.
17. **Setup item scale factors**: the `0.01` passed to `SetupItem::SetupItem` for the percent
    items is read as "setup value × 0.01 → member". The `SetupItem` class itself was not read.
18. **Choice made**: the brief listed `[DOWNSHIFT_PROFILER]`, `[THROTTLE_LUT]`, `[COAST_DATA]`,
    `[COAST_CURVE]` and `ALTITUDE_SENSITIVITY` as things to map. None of them is read by acs.exe;
    section 4 says so instead of inventing a mapping. (Re-checked in the review with
    `re_query.py uses`: no function holds any of these literals.)
19. **`Car::torqueModeEx == reactionTorques`** (review): no writer of that value and no reader of
    a `TORQUE_MODE_EX` key was found, so the wheel reaction-torque branch of 5.2 step 13 (and the
    matching branches in `Tyre::step` / `Tyre::addTyreForceToHub`) is treated as dead in this
    build. Unverified against raw-offset writes; a whole-image search for stores to Car+0x3d10
    would settle it.
20. **AWD2 and the `REAR_DIFF_*` setup items** (review): they are bound to `awdRearDiff`, which
    `step4WD_new` does not read. Checked against the data in the second review (4.7): only
    `ks_alfa_romeo_155_v6` exposes rear differential items, and they are the ineffective
    `REAR_DIFF_POWER` / `REAR_DIFF_COAST`. Still open: whether the setup screen or a setup-file
    loader remaps item names before `SetupManager` sees them (`SetupScreen::loadINI` was not
    read), which would be the only way for those two sliders to reach `diffPowerRamp` /
    `diffCoastRamp`.
21. **`[DOWNSHIFT_PROTECTION] DEBUG`** (review): overwritten unconditionally by
    `physicsGUID == 0` (4.2). So the debug print "DSP: Car…" is always on for car 0 and off for
    every other car, whatever the ini says.
22. **Push-to-pass activations** (review): the loader sets `p2p.activations = 0`; the count is
    filled from the game side (a `CarAvatar` lambda: `baseActivations + gridPos /
    basePositionCoeff`, capped at `maxActivations`; `ACClient::onMessageTCP` online). 0/113 cars
    use it, so it was not followed further.
23. **Not re-checked in either review**: `Curve::getValue`,
    `RaceEngineer::getDrivingTyresSlip`, the `CarAvatar::cycleERS*` bodies, and the
    disassembly-level operation order of `step4WD` (pseudo-C only). `ThermalObject::step` and the
    `step4WD_new` front update were read in the second review (5.9, question 5).
24. **`ctrl_single_lock.ini` on the two `AWD2` cars** (second review): shipped in
    `ks_porsche_cayenne` and `ks_porsche_macan` but never opened, because
    `Drivetrain::initControllers` 0x140267070 only looks for it when `tractionType == RWD`. Their
    rear differential therefore runs on the fixed `[AWD2] REAR_DIFF_*` values. Read from the
    pseudo-C branch structure; not confirmed by running the game.
25. **Engine blow-up on impact** (second review): the 150 threshold in
    `Car::onCollisionCallBack` is read from the code, but the two tests that can suppress it
    (a virtual call at +0x18 on each colliding shape returning 1 or 0x10; probably a collider
    group or type) were not interpreted.
