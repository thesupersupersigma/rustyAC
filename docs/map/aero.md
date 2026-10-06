# Aerodynamics

Source: `acs.exe` + `acs.pdb`, read only through the local material: pseudo-C in `re/decomp/`
(folders `AeroMap`, `Wing`, `DynamicWingController`, `DRS`, `DRSManager`, `SlipStream`, plus single
functions from `PhysicsEngine`, `Car`, `RaceEngineer`, `RaceManager`), struct layouts in `re/types/`,
`tools/disasm.py` for constants and loop counts, `tools/ini_survey.py` over the 113 cars in `cardata/`,
and the commented example car in the game's `sdk` folder. Copies of every function cited here are in
`re/car/aero/` (git-ignored). Nothing in the game folder, the Ghidra project or git was changed.
No Rust was written.

Size of the area: 37 hand-written functions, about 28 KB of code
(`AeroMap` 11, `Wing` 10, `DynamicWingController` 5, `DRS` 2, `DRSManager` 5, `SlipStream` 4).

Words used below: **confirmed** = read directly in the pseudo-C or disassembly. **Interpretation** =
my reading of what the code is for.

---

## 1. Plain-English summary

A car's aerodynamics is a list of flat "wings", each sitting at one fixed point of the car body.
The car body itself is just another wing (the first one, called BODY, on 111 of the 113 cars), and so are the
diffuser and, on a few cars, a vertical tail fin.
Every physics step each wing works out how fast the air is passing over its own point: the speed
of that point of the car plus a share of the wind, seen from the car's own point of view.
From that it gets two angles, how much the air comes from below or above (pitch) and how much from
the side (yaw), and adds the wing's own tilt angle.
A lookup table turns that angle into a drag number and a lift number, and a second optional table
scales them by how high the wing is above the road, which is how ground effect is done.
Body damage from crashes makes the drag number bigger and the lift number smaller.
The wing then pushes on the car body at its own point: drag straight against the airflow, lift
at right angles to it (downwards for a normal wing, sideways for a fin).
The tilt angle can be changed while driving: by the setup, by "controllers" that follow speed,
pedals, steering or g-forces (active aero), and by DRS, which flattens chosen wings when the driver
presses the button inside an allowed zone of the track.
Air density depends on the air temperature, and it drops by up to a quarter when the car sits in
the wake cone that every other car drags behind it (slipstream).
There is also an older, simpler format with one drag and one lift number for the whole car; the
code is still there, but none of the 113 cars use it.

---

## 2. Classes

| Class | Size | What it is for |
|---|---|---|
| `AeroMap` | 0x70 | One per car (`Car::aeroMap`, Car+0xab8). Owns the wing list and the current air density. Also holds the old whole-car numbers (`CD`, `CL`, `referenceArea`, `frontShare`, `CDX`, `CDY`, `CDA`). |
| `Wing` | 0x310 | One aerodynamic surface: data, live status, its controllers, damage factors, DRS override. Stored by value in `AeroMap::wings`. |
| `WingData` | 0x250 | Loaded numbers of a wing: name, chord, span, `area`, `position`, four `Curve`s (`lutAOA_CL`, `lutAOA_CD`, `lutGH_CL`, `lutGH_CD`), `clGain`, `cdGain`, `yawGain`, `isVertical`, `hasController`. |
| `WingState` | 0x44 | Live values of a wing: `aoa`, `yawAngle`, `cd`, `cl`, `angle`, `inputAngle`, `angleMult`, `groundHeight`, `groundEffectLift/Drag`, `dragKG`, `liftKG`, `liftVector`, `frontShare`, `isVertical`. Copied out for telemetry, replay and apps by `AeroMap::getWingStatus`, which replaces `angle` with `Wing::getCurrentModifiedAngle()` and fills `isVertical` from `data.isVertical` in the copy (the live `status.isVertical` stays false). |
| `WingOverrideDef` | 0x8 | `overrideAngle` + `isActive`: a forced wing angle (set by DRS in angle mode). |
| `DynamicWingController` | 0xb0 | One active-aero stage: an input channel, a lookup table, a smoothing filter, limits, and how to combine with the angle so far (`eAdd` = 1, `eMult` = 2). |
| `DRS` | 0x30 | One per car (`Car::drs`, Car+0x3640). Button logic and the list of wings it acts on. |
| `DRSWingConnection` | 0x18 | One DRS-controlled wing: `wing` pointer, `effect`, `angle`, `mode` (0 = UseEffect, 1 = UseAngle). |
| `DRSManager` | 0x38 | One per track (`Track::drsMamanger`, spelled like that in the PDB). Holds the DRS zones and answers "is this car inside one". |
| `DRSZone` | 0xc | `detection`, `start`, `end`, as fractions of the lap (0..1). |
| `DRSDetection` / `DRSDetectionStatus` | 0x20 / 0x10 | Per-car bookkeeping for race DRS rules (interpretation from the member names). Only read by `DRSManager::wasRaceSwitchedOn`; no per-step code that fills it was found (see Open questions). |
| `SlipStream` | 0x68 | One per car (`Car::slipStream`, Car+0x3ca8). The wake cone behind the car: tip position, direction, length, gains. Registered in `PhysicsEngine::slipStreams`. |
| `Wind` | 0x14 | `PhysicsEngine::wind`: `vector` (m/s), `speed` (a `Speed`, stored in m/s), `directionDeg`. |
| `RaceEngineer` | 0x18 | Small helper object every wing owns; used here for the ground-height and front-share geometry. |
| `Curve` | 0x80 | Shared lookup table (linear interpolation, clamped at both ends). Already ported (`crates/rustyac-physics/src/curve.rs`). |

Not physics, only listed: `SCarStateAero` (CD / CL_Front / CL_Rear in the car state), `WingAnimation`
and `wing_animations.ini` (graphics: moves the 3D wing from `status.angle`), `FormWings`,
`FormDRSZones` (developer apps), `ReplayFrameWingStatus`, `AIWingSetup` (AI picks wing setup values).

---

## 3. Key functions

### Loading / init

```
Car::Car                                          0x14026bf00
├─ (constructor defaults) aeroMap zeroed, slipStream.speedFactorMult = effectGainMult = 1,
│                         slipStream.speedFactor = 0.25 (replaced by 0.5 in SlipStream::init below),
│                         slipStreamEffectGain = 1, drs.isAvailable = true, drs.ignoreZones = false,
│                         drs.limitG = 0
├─ Car::initAeroMap                               0x140272a80
│  ├─ suspensions[0] / suspensions[2] ->getBasePosition (slot +0x50), x and y zeroed
│  │                                              (passed on but never used, see 5.5)
│  ├─ AeroMap::init                               0x1402b5ca0   defaults (airDensity = 1.221 until the first step), then:
│  │  └─ AeroMap::loadINI                         0x1402b5d50   reads aero.ini
│  │     ├─ [SLIPSTREAM] -> car->slipStream gains
│  │     ├─ loop WING_0, WING_1, ... : Wing::Wing(car, ini, i, vertical=false)   0x1402b1340
│  │     ├─ loop FIN_0,  FIN_1,  ... : Wing::Wing(car, ini, i, vertical=true)    (appended after the wings)
│  │     │     ├─ WingData::WingData              0x1402b22c0   zero defaults
│  │     │     ├─ Curve::load x2..4               0x140206a50   the .lut files
│  │     │     └─ RaceEngineer::getPointFrontShare 0x14027c4d0  status.frontShare (display only)
│  │     ├─ no wings at all: read [DATA] (old format)
│  │     └─ loop DYNAMIC_CONTROLLER_0, ... : DynamicWingController::DynamicWingController 0x1402a9df0
│  │           └─ DynamicWingController::initCommon 0x1402aa0d0  pushed into wings[WING].dynamicControllers
│  └─ DRS::init                                   0x1402b4330   reads drs.ini
├─ damageZoneLevel[0..4] = 0
└─ SlipStream::init                               0x1402aadc0   registers in PhysicsEngine::slipStreams,
                                                                speedFactor = 0.5, length = 0

Track::Track                                      0x140277100
└─ DRSManager::DRSManager                         0x140278ea0   reads <track>/data/drs_zones.ini

RaceManager::initOffline                          0x14013a6c0   session ini [TEMPERATURE] AMBIENT -> ambientTemperature,
└─ RaceManager::generateWind                      0x140137ba0   [WIND] -> queued to the physics thread
   └─ lambda                                      0x140133ac0
      └─ PhysicsEngine::setWind                   0x1402645a0
ACClient::onMessageTCP                            0x14004c840   online: server sends wind -> setWind
DrivingAssistManager::DrivingAssistManager        0x1400fbd90   offline only: cfg/assists.ini [ASSISTS] SLIPSTREAM (and DAMAGE)
└─ CarAvatar::setSlipStreamEffects (each car)     0x1400da740   wake length factor of the non-player cars, see 5.2
ACClient::onDRSZoneReceived                       0x14004b520   online: server sends zones -> DRSManager::setZones 0x140279500
```

### Every physics step

```
PhysicsEngine::step(dt = 0.003)                   0x140264760
├─ Track::step                                    0x140278d20   calls an empty function on the DRSManager (0x140017870)
├─ PhysicsEngine::stepWind                        0x140265380   slow +-10 % gust on the wind vector
├─ Car::step (each car)                           0x140275da0
│  ├─ Car::updateAirPressure                      0x140276ae0   early in Car::step, before fuel use
│  │  ├─ PhysicsEngine::getAirDensity             0x140263a60
│  │  └─ SlipStream::getSlipEffect (each other car) 0x1402aac60
│  └─ Car::stepComponents                         0x1402764d0
│     ├─ ... BrakeSystem, EDL, suspensions, Tyre::step x4, HeaveSpring::step x2 ...
│     ├─ DRS::step                                0x1402b4e60
│     │  ├─ DRSManager::isDRSAvailable            0x140279490
│     │  └─ Wing::setOverrideAngle 0x1402b2bb0 / Wing::clearOverrides 0x1402b2b70 / writes status.angleMult
│     ├─ AeroMap::step                            0x1402b7150
│     │  ├─ (only when there are no wings)  body->getLocalVelocity, AeroMap::addDrag 0x1402b5860, AeroMap::addLift 0x1402b5a90
│     │  └─ Wing::step (each wing, then each fin) 0x1402b2bc0
│     │     ├─ Wing::stepDynamicControllers       0x1402b2dd0   (skipped while a DRS angle override is active)
│     │     │  └─ DynamicWingController::step     0x1402aaa70
│     │     │     ├─ DynamicWingController::getInput 0x1402a9ef0
│     │     │     └─ Curve::getValue              0x140206990
│     │     ├─ body->getLocalPointVelocity(position)        (slot +0x100)
│     │     ├─ Car::getGroundWindVector           0x140270c00
│     │     ├─ body->worldToLocalNormal                     (slot +0x68)
│     │     ├─ body->localToWorld(position)                 (slot +0x50)
│     │     ├─ RaceEngineer::getPointGroundHeight 0x14027c550
│     │     ├─ Wing::addDrag                      0x1402b2420   -> body->addLocalForceAtLocalPos (slot +0xf0)
│     │     └─ Wing::addLift                      0x1402b2730   -> body->addLocalForceAtLocalPos (slot +0xf0)
│     └─ Kers, ERS, SteeringSystem, ... (rest of the list)
├─ core->step  (ODE integrates the forces)
└─ evOnStepCompleted handlers
   └─ Car::postStep                               0x140275430   reached from a callback wrapper at 0x14026ef00
      └─ SlipStream::setPosition                  0x1402aae80   moves this car's wake cone
```

Read-only users of the results (not part of the step): `AeroMap::getWingStatus` (`0x1402b5bc0`),
`AeroMap::getCurrentDragKG` (`0x1402b5b60`),
`AeroMap::getCurrentLiftKG` (`0x1402b5b90`),
`Wing::getCurrentModifiedAngle` (`0x1402b2b80`),
`Car::getWingState` (`0x140272a20`).

Not in the trees because they have no behaviour of their own: the second
`DynamicWingController` constructor (`0x1402a9e70`, takes a `CarPhysicsState` instead of a `Car`,
used by remote cars), the copy constructors `Wing::Wing(const Wing&)` (`0x1402b5260`) and
`DynamicWingController(const DynamicWingController&)` (`0x1402b51f0`) that `push_back` uses in
`AeroMap::loadINI`, and the destructors.

---

## 4. Data it reads

Missing keys: `INIReader::getFloat` (`0x1402358c0`) returns 0.0 for a key that is not there, so
"default when missing" below is 0 unless something else is written. All keys were confirmed in
the pseudo-C of the loader named in each heading.
"Crash" below means `ksGenerateCrash`. In `DynamicWingController::initCommon` it is behind the
global `INIReader::crashAtError`, which `wWinMain` (`0x1400c29c0`) sets from
`system/cfg/assetto_corsa.ini` `[ASSETTO_CORSA] WARNINGS_AS_ERRORS` (0 in the installed file); the
crashes in `AeroMap::loadINI` and `DRS::init` are unconditional.

### 4.1 aero.ini (all 113 cars) - `AeroMap::loadINI` and `Wing::Wing`

| Section / key | Goes to | Default | Notes |
|---|---|---|---|
| `[HEADER] VERSION` | local in `Wing::Wing` | 0 | Switches: `>= 2` reads the `ZONE_*` damage keys and sets `hasDamage`; `>= 3` reads `YAW_CL_GAIN`. Cars: 68 have 2, 45 have 3. |
| `[WING_n]` | one `Wing`, horizontal | - | n = 0, 1, 2 ... until a section is missing. 1 to 18 per car. |
| `[FIN_n]` | one `Wing` with `data.isVertical = true` | - | Same keys as `WING_n`. Appended after all wings, so fin 0 has index "number of wings". 4 cars: `ks_ferrari_f2004`, `f2004ButBetter`, `ks_ferrari_sf70h`, `lotus_98t`. |
| `NAME` | `data.name` | "" | Label only (BODY, FRONT, REAR, DIFFUSER ...). The code never looks at it. |
| `CHORD`, `SPAN` | `data.chord`, `data.span`; `data.area = chord * span` | 0 | metres; only the product is used. |
| `POSITION` | `data.position` (vec3f) | 0,0,0 | metres, car body axes from the origin of `Car::body` (x sideways with +x = the car's left, see 6; y up; z forward). The ini comment in the example car says "starting from the CoG"; an ODE body's origin is its centre of mass, and the fuel tank is a separate body. This is both where the air speed is measured and where the forces are applied. |
| `LUT_AOA_CL` | `data.lutAOA_CL` | no existence check | File in the car data folder (`content/cars/<car>/data/<file>` through `Car::getConfigPath` `0x140270420`, which swaps in `content/cars/<car>/data_<configName>/<file>` when the car was created with a `configName` and that file exists; see `docs/map/body.md`. aero.ini and drs.ini themselves go through the same function). x = angle in degrees, y = lift coefficient (positive = downforce). |
| `LUT_AOA_CD` | `data.lutAOA_CD` | no existence check | x = angle in degrees, y = drag coefficient. |
| `LUT_GH_CL` | `data.lutGH_CL` | empty curve | Loaded only when the file exists (`Path::fileExists`). x = ground height in metres, y = multiplier on CL. Empty value on 272 of the 427 `WING_n` sections in `cardata/`. |
| `LUT_GH_CD` | `data.lutGH_CD` | empty curve | Same, multiplier on CD. |
| `CL_GAIN`, `CD_GAIN` | `data.clGain`, `data.cdGain` | 0 | Plain multipliers. |
| `ANGLE` | `status.angle` and `status.inputAngle` | 0 | degrees. Start value; later changed by setup, controllers, DRS. |
| `ZONE_FRONT_CL`, `ZONE_REAR_CL`, `ZONE_LEFT_CL`, `ZONE_RIGHT_CL` | `damageCL[0..3]` | 0 | VERSION >= 2 only. |
| `ZONE_FRONT_CD`, `ZONE_REAR_CD`, `ZONE_LEFT_CD`, `ZONE_RIGHT_CD` | `damageCD[0..3]` | 0 | VERSION >= 2 only. The loop runs 4 times (confirmed in the disassembly); a fifth name "CENTER" is built but never read, so `damageCL[4]` / `damageCD[4]` stay unset and unused. |
| `YAW_CL_GAIN` | `data.yawGain` | 0 | VERSION >= 3 only (45 cars). Usually 0 or slightly negative. |
| `[DYNAMIC_CONTROLLER_n] WING` | index into `AeroMap::wings` (fins count too) | - | Out of range prints an error and skips the controller. Sets `data.hasController` on that wing. 37 cars. |
| `... INPUT` | `inputVar` | `eUndefined` (0): input reads as 0.0 | `BRAKE` (1), `GAS` (2), `LATG` (3), `LONG` (4), `STEER` (5), `SPEED_KMH` (6), `SUS_TRAVEL_LR` (7), `SUS_TRAVEL_RR` (8). Anything else prints "ERROR: INPUT UNDEFINED" and, only when `INIReader::crashAtError` is set, "Undefined controller requested" + `ksGenerateCrash`. With the installed config the stage is kept and `getInput` returns 0.0 for it, so its output is `lut(0)`. Used in data (121 stages): SPEED_KMH 67, BRAKE 19, GAS 17, LATG 8, STEER 6, and the unknown `SCRIPT_12` 4; `LONG` and the suspension-travel inputs are used by no car. |
| `... COMBINATOR` | `combinatorMode` | `eUndefinedMode` (0), error printed | `ADD` (1) or `MULT` (2). An undefined stage neither adds nor multiplies, but its `UP_LIMIT` / `DOWN_LIMIT` clamp still runs on the angle so far. |
| `... LUT` | `lut` | no existence check | `content/cars/<car>/data/<file>` passed straight to `Curve::load` (not through `Car::getConfigPath`, unlike the wing tables); x = input value, y = angle in degrees (ADD) or factor (MULT). |
| `... FILTER` | `filter = (1 - FILTER) * 1.3333334 * 333.33334` | - | 0 = no smoothing, values near 1 = slow. |
| `... UP_LIMIT`, `DOWN_LIMIT` | `upLimit`, `downLimit` | 0 | degrees; clamp applied after this stage. |
| `[SLIPSTREAM] EFFECT_GAIN_MULT` | `car->slipStream.effectGainMult` | 1.0 (section missing) | 2 cars (both `vrc_formula_alpha_2026*`). |
| `[SLIPSTREAM] SPEED_FACTOR_MULT` | `car->slipStream.speedFactorMult` | 1.0 (section missing) | Same 2 cars. |
| `[DATA] REFERENCE_AREA`, `CD`, `CL`, `FRONT_SHARE`, `CDX`, `CDY` | `AeroMap::referenceArea`, `CD`, `CL`, `frontShare`, `CDX`, `CDY` | 1.0, 0, 0, 0.5, 0, 0 before the read | Old format. Read only when there is no `WING_n` / `FIN_n`. **0 of 113 cars.** |

Format rules enforced by `AeroMap::loadINI` (both end in `ksGenerateCrash`):
no wings and no `[DATA]` -> "aero.ini does not contain WINGS nor DATA";
wings and `[DATA]` together -> "aero.ini contains DATA with WINGS, which is redundant".

Two cars (`vrc_formula_alpha_2026` and `vrc_formula_alpha_2026_csp`) use `INPUT=SCRIPT_12` (two
stages each), and the `_csp` one also has a `[MAP_n]` section with `.2Dlut` files. No function in
acs.exe reads those names: they belong to an external mod loader. This exe prints the undefined-input
error and runs those stages with input 0.0 (it only stops when `WARNINGS_AS_ERRORS` is on).

### 4.2 The .lut files

All are plain `x|y` text lines read by `Curve::load`, looked up with `Curve::getValue` (linear,
clamped to the first / last point).

The first number is how many car folders contain the file (`ini_survey.py --files`); the number
in brackets is how many cars' `aero.ini` actually name it in a `WING_n` / `FIN_n` / controller
section (own count over `cardata/`, CL file / CD file where they differ). Leftover files are common.

| File pattern | Used as | Cars with the file (cars that reference it) |
|---|---|---|
| `wing_body_AOA_CL/CD.lut` | body lift / drag vs angle | 111 (111) |
| `wing_front_AOA_CL/CD.lut`, `wing_rear_AOA_CL/CD.lut` | front / rear wing | 104 (102 / 101), 101 (100 / 97) |
| `wing_diffuser_AOA_CL/CD.lut` | diffuser | 43 (42) |
| `wing_bodylift_AOA_*.lut`, `fin_AOA_*.lut`, other names | extra surfaces, fins | 6 (4), 4 (4), few |
| `height_front_CL/CD.lut`, `height_frontwing_CL/CD.lut` | ride-height multiplier of front surfaces | 46 (44 / 42), 17 (14 / 15) |
| `height_diffuser_CL/CD.lut`, `height_diffuser_rear_*`, `height_body_CD.lut` | ride-height multiplier of diffuser / body | 44 (41), 7 (7), 6 (6) |
| `wing_controller_speed.lut`, `_gas`, `_brake`, `_highspeed`, `_latg`, `_long`, `_steer_diff_fr` | controller tables (input -> angle or factor) | 29 (28), 11 (6), 5 (4), 5 (5), 4 (1), 3 (0), 5 (5) |

Example (`ks_ferrari_f2004`): 6 wings (BODY, FRONT, REAR, DIFFUSER_F, DIFFUSER_RL, DIFFUSER_RR),
1 fin, 6 controllers. (`ks_ferrari_sf70h` is the only car with two fins.) `height_frontwing_CL.lut` starts `0|0`, `0.010|0.5`, `0.020|1.00`,
`0.030|1.06`: the height axis is metres and the value multiplies CL.

### 4.3 drs.ini - `DRS::init`

The file exists in 73 cars but is an empty placeholder in most; only 7 have a `[WING_n]` section.

| Section / key | Goes to | Default | Notes |
|---|---|---|---|
| `[HEADER] VERSION` | local | 1 | `> 1` enables `MODE`. 1 car. |
| `[DRS_ZONES] IGNORE_ZONES` | `DRS::ignoreZones` | false | Non-zero = DRS allowed everywhere. 2 cars. |
| `[DEACTIVATION] LIMIT_G` | `DRS::limitG` | 0 (off) | Lateral g above which DRS closes. 1 car. |
| `[WING_n] EFFECT` | `DRSWingConnection::effect` | 0 | n is the index in `AeroMap::wings` (fins included); the loader only tries n below the number of wings. It tests every such n with `hasSection`, so a gap does not end the loop (unlike the `WING_n` loop of aero.ini): `ks_ferrari_f138` has only `[WING_7]`. A section here is what makes `DRS::isPresent` true. The whole file is skipped when it cannot be opened (`INIReader::ready` false). |
| `[WING_n] MODE` | `mode` | `EFFECT` when VERSION <= 1 | Read only when VERSION > 1, and then it must be exactly `EFFECT` or `ANGLE`: anything else, a missing key included (empty string), is a crash ("Unknown DRS mode"). |
| `[WING_n] ANGLE` | `angle` | 0 | Only read when `MODE=ANGLE`. |

### 4.4 Track and session data

| File | Key | Goes to | Notes |
|---|---|---|---|
| `<track>/data/drs_zones.ini` | `[ZONE_n] DETECTION`, `START`, `END` | `DRSManager::zones` | Lap fractions 0..1. No file = no zones = DRS allowed everywhere. |
| Documents `cfg/race.ini`, read by `RaceManager::initOffline` (`0x14013a6c0`, its argument is an `INIReaderDocuments &`); the same block also reads `[TEMPERATURE] ROAD` into `PhysicsEngine::roadTemperature` (used by the tyre thermal model) | `[TEMPERATURE] AMBIENT` | `PhysicsEngine::ambientTemperature` | deg C. Engine default 26. |
| same | `[WIND] SPEED_KMH_MIN`, `SPEED_KMH_MAX` | `RaceManager::windSettings.baseSpeed` | Each clamped to 0..40, then one random value between them. |
| same | `[WIND] DIRECTION_DEG` | `windSettings.baseDirection` | Negative = random 0..360. |
| `cfg/assists.ini`, read by `DrivingAssistManager::DrivingAssistManager` (`0x1400fbd90`) through `INIReaderDocuments` (`0x140238350`): the copy under the user's documents folder (base path set up from `system/cfg/inireaderdocuments.ini`), not the one in the install folder. These two keys are read offline only (`sim->client == null`) | `[ASSISTS] SLIPSTREAM` | S = max(value, 1.0), then `slipStream.speedFactor` = 0.5 for car 0 and 0.5 * S for every other car | Through `CarAvatar::setSlipStreamEffects` (`0x1400da740`). The install folder's template `cfg/assists.ini` has 1. See 5.2. |
| same | `[ASSISTS] DAMAGE` | `PhysicsEngine::mechanicalDamageRate = DAMAGE * 0.01` | Integer percent (`getInt`). 0 switches wing damage off (5.4); the install folder's template has 0. Online the rate comes from the server (`ACClient::handshakeTCP` `0x140048b50`). |
| server message `'x'` in `ACClient::onMessageTCP` (`0x14004c840`), online | - | `ambientTemperature`, `roadTemperature`, then `setWind` | Temperatures arrive as unsigned bytes (whole deg C). Wind speed is a 16-bit km/h value, clamped to at most 40 (and to a lower bound held in a register, taken to be 0) and put through `Speed::fromKMH`; direction is a second 16-bit value in degrees. Both go straight to `setWind`: the offline random spread of 5.1 is not applied here (read in the disassembly, review). |

---

## 5. Each step: inputs, maths, outputs

Car body axes used everywhere below: x sideways, y up, z forward (confirmed by the code: lift is
zeroed when `v.z < 0`, front wings have positive z). All maths is 32-bit float.

### 5.1 Wind - `PhysicsEngine::setWind` and `PhysicsEngine::stepWind`

`setWind(speed, directionDeg)`: `wind.vector` = (0, 0, speed) rotated about the world Y axis by
`-(directionDeg * 0.017453)` radians; `wind.speed = speed`; `wind.directionDeg = directionDeg`.
`Speed` holds metres per second (`Speed::fromKMH` multiplies by 0.2777778).

Offline the lambda behind `RaceManager::generateWind` picks, only when `baseSpeed > 0`:
`a = base * 0.2`, `speedKMH = r1 * (a - (-a)) + (-a) + base` (on paper `base * (0.8 + 0.4 * r1)`;
the order written is the one in the disassembly), `direction = baseDirection + (r2 * 40 - 20)`,
with `r = (float)rand() * 3.0518509e-05`. The speed is converted with `Speed::fromKMH` before `setWind`.

`stepWind` (every step, constants confirmed in the disassembly):

```
if wind.speed.value >= 0.01:
    s   = wind.speed.value * (1.0 + (float)sin((physicsTime - sessionInfo.startTimeMS) * 0.0001) * 0.1)
    dir = wind.vector / |wind.vector|          (left as is when the length is 0)
    new = dir * s
    if new.x, new.y, new.z are all finite:  wind.vector = new
```

So the wind strength swings +-10 % with a period of about 63 seconds; the direction never changes.

### 5.2 Air density and slipstream - `Car::updateAirPressure`

Runs once per car per step in `Car::step`, before `Car::stepComponents`.

```
rho = 1.2922 - ambientTemperature * 0.0041              PhysicsEngine::getAirDensity
if slipStreamEffectGain > 0:
    p = body->getPosition(0)                            car body origin, world
    m = 1.0
    for every SlipStream s in ksPhysics->slipStreams except this car's own:
        f = clamp(1.0 - s.getSlipEffect(p) * slipStreamEffectGain, 0, 1)
        if f < m:  m = f
    rho = (rho - m * rho) * (0.75 / slipStreamEffectGain) + m * rho
aeroMap.airDensity = rho
```

`slipStreamEffectGain` is only ever set to 1.0 (three writers, all write 1.0: `Car::Car`,
`Car::setSlipStreamEffects`, `CarAvatar::setSlipStreamEffects`). With that this is `rho * (0.75 + 0.25 * m)`: full slipstream removes at
most 25 % of the air density. There is no altitude or pressure term anywhere.
Before the first `Car::step` the value is 1.221, set by `AeroMap::init`.

`SlipStream::getSlipEffect(p)` (the wake of one other car):

```
d = p - triangle.points[0];  dist = |d|
if dist < length:
    c = normalize(d) . dir
    if c > 0.7:  return (1 - dist / length) * (c - 0.7) * 3.3333333 * effectGainMult
return 0
```

`SlipStream::setPosition(pos, vel)`, called after the solver from `Car::postStep` with the body
position and world velocity (and from `NetCarStateProvider::step` `0x14011bd00` for remote cars,
whose wakes are in the same `slipStreams` list and so slow the local car's air too):

```
points[0] = pos
length    = |vel| * speedFactor * speedFactorMult        speedFactor is 0.5 (0.5 * S on non-player cars, below)
dir       = normalize(-vel)
h         = normalize((vel.x, 0, vel.z));  side = (-h.z, 0, h.x) * length * 0.25
points[1] = pos - h * length + side;  points[2] = pos - h * length - side      (only the cone outline, not used by getSlipEffect)
```

Interpretation: the wake is a cone behind the car, half-angle about 45 degrees (cos > 0.7),
as long as half the car's speed in metres (50 m at 100 m/s; times `SPEED_FACTOR_MULT`, 2.24 on the
two cars that set it), strongest right behind the car on its line of travel. Because `setPosition` runs after the step, a car reads the other cars' wakes
from the end of the previous step. That also makes the result independent of the order in which
the cars are stepped: `PhysicsEngine::step` can hand `Car::step` of different cars to its own
thread pool (see `docs/map/physics_engine.md`), and no wake is written while the cars are stepping.

`Car::setSlipStreamEffects(receive, speedFactor)` (`0x140275d40`) sets `slipStreamEffectGain = 1.0`
whatever it is given and `slipStream.speedFactor = speedFactor * 0.5`. It has no direct caller; the
copy that runs is `CarAvatar::setSlipStreamEffects` (`0x1400da740`, the same two writes on
`CarAvatar::physics`). `DrivingAssistManager::DrivingAssistManager` (`0x1400fbd90`) calls it once per
car, offline only, with S = max(`cfg/assists.ini [ASSISTS] SLIPSTREAM`, 1.0): `(S, 1.0)` for car 0
and `(1.0, S)` for every other car. Because `receive` is ignored, the setting does not change how
much density the player loses; it makes the wakes of the other cars S times longer
(`speedFactor = 0.5 * S`). Online this block is skipped and `speedFactor` stays 0.5.

### 5.3 DRS - `DRS::step`

Runs just before `AeroMap::step`. Does nothing unless `isPresent`.

```
isAvailable = ignoreZones || DRSManager::isDRSAvailable(car)
if controls.drs && !lastState && isAvailable:  isActive = !isActive        button press toggles
lastState = controls.drs
if !isAvailable || controls.brake > 0.1 || (limitG > 0 && |accG.x| > limitG):  isActive = false
for each connection c:
    mode 0 (effect):  c.wing->status.angleMult = isActive ? c.effect : 1.0
    mode 1 (angle):   isActive ? c.wing->setOverrideAngle(c.angle) : c.wing->clearOverrides()
```

`DRSManager::isDRSAvailable(car)`: true when the track has no zones; otherwise true when
`car->splineLocatorData.npos` lies in any zone. Normal zone: `start <= npos <= end`. Zone that
wraps the start line (`end <= start`): `npos >= start` when `npos >= 0.5`, else `npos <= end`.
The `DETECTION` value and any "within one second of the car ahead" rule are **not** used here.

Effect on the wing: the table angle is `angleMult * angle + aoa`, so `EFFECT=0` makes the wing
behave as if set to 0 degrees. In angle mode `status.angle` is replaced by `overrideAngle` for the
step (and the wing's controllers are skipped), still multiplied by `angleMult` (1.0 unless another
connection changed it).

### 5.4 Wings - `Wing::step`, `Wing::addDrag`, `Wing::addLift`

`AeroMap::step` calls `Wing::step` for every element of `wings` in file order.

**Active aero first** (`Wing::stepDynamicControllers`, only when the wing has controllers and no
override is active):

```
a = status.inputAngle
for each controller c of this wing, in file order:
    DynamicWingController::step(c):
        target = c.lut(getInput(c))
        if |target - c.outputAngle| >= 0.001:
            k = clamp(c.filter * 0.003, 0, 1)                the 0.003 is a literal, not dt
            c.outputAngle = (target - c.outputAngle) * k + c.outputAngle
        else c.outputAngle = target
    if c.combinatorMode == eAdd:  a = a + c.outputAngle
    if c.combinatorMode == eMult: a = a * c.outputAngle
    a = (a > c.upLimit) ? c.upLimit : (a < c.downLimit ? c.downLimit : a)
status.angle = a
```

`k` works out to `(1 - FILTER) * 1.3333`: FILTER 0.99 moves 1.3 % of the gap per step, FILTER 0 jumps
(any FILTER below about 0.25 gives k >= 1 and so jumps).
`outputAngle` starts at 0.0 (`DynamicWingController::initCommon`), not at the table value, so a
filtered stage ramps up from 0 during the first steps after loading: a `MULT` stage starts by
multiplying the angle by about 0, an `ADD` stage by adding about 0.
Inputs (`DynamicWingController::getInput`): `controls.brake`, `controls.gas`, `accG.x` (LATG),
`accG.z` (LONG), `controls.steer`, `Car::getSpeed() * 3.6` (km/h), rear-left / rear-right
suspension `getStatus()->travel * 1000` (mm).

**Air speed, angles, height:**

```
vW = body->getLocalPointVelocity(data.position)      world velocity of the wing's point (dBodyGetRelPointVel)
vW = vW + Car::getGroundWindVector()                 see below
v  = body->worldToLocalNormal(vW)                    into car axes
status.groundHeight = RaceEngineer::getPointGroundHeight(body->localToWorld(data.position))
saved = status.angle                                 (after the controllers ran)
if overrideStatus.isActive:  status.angle = overrideStatus.overrideAngle
if v.z == 0:   aoa = cd = yawAngle = cl = 0, no force (dragKG, liftKG, liftVector, groundEffect* keep their old values)
else:
    status.aoa      = atanf(v.y * (1 / v.z)) * 57.29578      degrees, see the sign note below
    status.yawAngle = atanf(v.x * (1 / v.z)) * 57.29578      positive = the point moves towards +x
    addDrag(v);  addLift(v)
if overrideStatus.isActive:  status.angle = saved    (so between steps status.angle is never the override)
```

Sign of `aoa` (corrected in review): `v` is the velocity of the wing's point, not of the air, so
`aoa` is positive when the point moves **up** in car axes: nose-down pitch (rake) or the body
rising. The air then meets the wing from above, not from below. The data agrees:
`wing_body_AOA_CL.lut` of `ks_ferrari_f2004` is -0.2 at -10 degrees and +0.2 at +10 degrees, i.e.
a nose-up body lifts.

`Car::getGroundWindVector`: n = normal of the plane through `tyres[0..2].unmodifiedContactPoint`;
result = `(wind.vector - (wind.vector . n) * n) * 0.44`. The wind is flattened onto the road and
only 44 % of it is used. It is **added** to the car's velocity (confirmed), so `wind.vector` acts as
the direction the wind blows from, not towards (interpretation).

`RaceEngineer::getPointGroundHeight(p)`: two planes, through `tyres[0], [1], [2].contactPoint` and
through `tyres[0], [1], [3].contactPoint`. For each: `den = n.x*0 + n.y*(-1) + n.z*0` and
`hitY = (den == 0) ? 0 : -((p.y*n.y + n.x*p.x + n.z*p.z + d) / den) * -1.0 + p.y`, the height at
which a ray straight down the world Y axis from p meets the plane. Returns
`((p.y - hitY2) + (p.y - hitY1)) * 0.5`, the average of the two distances. For a vertical or
degenerate plane (`den == 0`) the hit height is 0, so that plane contributes the world height `p.y`
itself, not 0.
`plane4f::plane4f(p1, p2, p3)` (`0x140117ed0`), used here and in `getGroundWindVector`:
`normal = normalize((p2 - p1) x (p3 - p1))`, `d = -(normal . p1)`; if the cross product has zero
length, normal and d stay 0. Both users give the same result for either sign of the normal.
So ground height = height of the wing's own `POSITION` above the road plane under the wheels, not a
suspension ride height. `Tyre::step` has already run this step, so the contact points are current.

**Drag** (`Wing::addDrag`):

```
a   = isVertical ? yawAngle : aoa
q   = v.x^2 + v.y^2 + v.z^2;   n = v / |v|
cd  = lutAOA_CD(angleMult * angle + a) * cdGain
if hasDamage && ksPhysics->mechanicalDamageRate > 0:
    e = 0
    for zone i = 0..3:  t = (SURFACE_DAMAGE_COEFF * damageCD[i]) * ((1 / SPEED_DAMAGE_COEFF) * car->damageZoneLevel[i])^2
                        if e <= t:  e = t
    cd = (clamp(e, 0, 1) + 1.0) * cd
if lutGH_CD has points:  groundEffectDrag = lutGH_CD(groundHeight);  cd = groundEffectDrag * cd
status.cd = cd
D = q * cd * car->aeroMap.airDensity * data.area * 0.5
status.dragKG = D * 0.101978384
if q != 0:  body->addLocalForceAtLocalPos(-D * n, data.position)
```

**Lift** (`Wing::addLift`):

```
a  = isVertical ? yawAngle : aoa
q  = (isVertical ? v.x : v.y)^2 + v.z^2               the third axis is left out
cl = lutAOA_CL(angleMult * angle + a) * clGain
if v.z < 0:  cl = 0                                    no lift when rolling backwards
if yawGain != 0 && !isVertical:
    cl = clamp(sinf(|yawAngle| * 0.017453) * yawGain + 1.0, 0, 1) * cl
if lutGH_CL has points:  groundEffectLift = lutGH_CL(groundHeight);  cl = groundEffectLift * cl
if hasDamage && mechanicalDamageRate > 0:
    e = same maximum as above but with damageCL[i]
    horizontal wing:  only if cl > 0:  cl = (e > 1) ? cl * 0 : (1 - max(e, 0)) * cl
    fin:              cl = (1 - clamp(e, 0, 1)) * cl
status.cl = cl
L = q * cl * car->aeroMap.airDensity * data.area * 0.5
status.liftKG = L * 0.101978384
if q != 0:
    n = v / |v|                                        all three components
    horizontal:  dir = (n.y*0 - n.z*0,  n.z - n.x*0,  n.x*0 - n.y)      = n cross (1,0,0)
    fin:         dir = (n.y*0 - n.z,    n.z*0 - n.x*0, n.x - n.y*0)     = n cross (0,1,0)
    status.liftVector = -L * dir
    body->addLocalForceAtLocalPos(status.liftVector, data.position)
```

Notes:
- `SPEED_DAMAGE_COEFF` and `SURFACE_DAMAGE_COEFF` are both 300.0, set in the constructor, never read
  from a file. With zone coefficient z and damage level d the effect is `300 * z * (d / 300)^2`,
  largest zone wins. This is not the formula written in the ini comments (`CL/(1+ZONE*DAMAGE)`).
  d is the hardest hit so far in that zone: relative speed along the contact normal in km/h times
  `mechanicalDamageRate` (`Car::onCollisionCallBack`, see 6). Example: zone coefficient 0.015 and a
  100 km/h hit give 300 * 0.015 * (1/3)^2 = 0.5, so half the lift is gone or drag is up 50 %.
- Order differs between the two: drag applies damage before ground effect, lift applies ground
  effect before damage. `status.cd` stored in between is overwritten by the final value.
- Positive CL pushes along -y when driving forward: positive = downforce. For a fin the table is odd
  (`fin_AOA_CL.lut`: +6 deg -> -0.27 on `ks_ferrari_f2004`, -0.67 on `ks_ferrari_sf70h`). A fin's
  force is `(L * n.z, 0, -L * n.x)`, so with positive yaw (moving towards +x) and negative CL it
  points to -x: a side force against the slide (sign worked out from the code in review).
- Body drag is not special: it is the wing called BODY (111 cars, always `WING_0`; `CL_GAIN` is 0 on 97 of them).
- Ground effect is only the `LUT_GH_*` multiplier. There is no separate ground-effect model.
- `Wing::getCurrentModifiedAngle` (what telemetry and the 3D wing show) returns `overrideAngle`
  alone while an override is active, but `angleMult * angle` otherwise; the force code multiplies
  the override by `angleMult` too. Only differs if one wing has both DRS modes attached.
- Forces go to `Car::body` only, through `IRigidBody` slot +0xf0 (`dBodyAddRelForceAtRelPos`):
  force and position both in body axes, so a wing away from the centre of mass also makes a torque.

### 5.5 Old format - `AeroMap::addDrag`, `AeroMap::addLift` (no car uses it)

Only when `wings` is empty. `v = body->getLocalVelocity()` (centre of mass, car axes, **no wind**).

```
addDrag:  q = |v|^2;  if q != 0:
    n = v / |v|
    dynamicCD = |n.x| * CD * CDX + CD + |n.y| * CD * CDY
    body->addLocalForce(-(dynamicCD * q * airDensity * referenceArea * 0.5) * n)
    w = body->getAngularVelocity();  if |w|^2 != 0:  body->addLocalTorque(-|w|^2 * CDA * w / |w|)     CDA = 0.1
addLift:  L = v.z^2 * CL * airDensity * referenceArea * 0.5;  if v.z^2 != 0:
    body->addLocalForceAtLocalPos((0, -(L * frontShare), 0),        frontApplicationPoint)
    body->addLocalForceAtLocalPos((0, -((1 - frontShare) * L), 0),  rearApplicationPoint)
```

The spin damping reads the world-frame angular velocity (`dBodyGetAngularVel`) and applies the
result as a body-frame torque (`dBodyAddRelTorque`); transcribed as found.

`frontApplicationPoint` and `rearApplicationPoint` are zeroed in `Car::Car` and written nowhere
else: `AeroMap::init` receives the front and rear axle positions from `Car::initAeroMap` but does not
store them (confirmed in the disassembly). So in this build both halves act at the body origin and
`FRONT_SHARE` has no effect. The spin damping (`CDA`) exists only in this path; wing cars have none.

### 5.6 What is written

| Written | By | Meaning |
|---|---|---|
| `PhysicsEngine::wind.vector` | `stepWind`, `setWind` | wind in m/s, world |
| `AeroMap::airDensity` | `Car::updateAirPressure` | kg/m3 after slipstream |
| `DRS::isAvailable`, `isActive`, `lastState` | `DRS::step` | DRS state |
| `WingState::angleMult`, `Wing::overrideStatus` | `DRS::step` | DRS effect on a wing |
| `DynamicWingController::outputAngle`, `WingState::angle` | controllers | active-aero angle |
| `WingState::aoa`, `yawAngle`, `groundHeight`, `cd`, `cl`, `groundEffectDrag/Lift`, `dragKG`, `liftKG`, `liftVector` | `Wing::step` | live values |
| forces on `Car::body` | `Wing::addDrag`, `Wing::addLift` | two force calls per wing per step |
| `SlipStream::triangle`, `dir`, `length` | `SlipStream::setPosition` | wake cone for the next step |
| `AeroMap::dynamicCD` | old path only | stays 0 on every real car; `dynamicCL` is never written at all |

---

## 6. Connections to other systems

### Inputs (who writes what this system reads)

| Input | Written by | Read in |
|---|---|---|
| `Car::body` velocity, rotation, position | ODE step (previous step) | `Wing::step`, `Car::updateAirPressure`, `Car::postStep` |
| `Tyre::contactPoint`, `Tyre::unmodifiedContactPoint` (all four) | `Tyre::step`, earlier in the same `Car::stepComponents` | ground height, ground wind plane |
| `Car::controls` (`gas`, `brake`, `steer`, `drs`) | controls polling / AI (`AIDriver::stepDRS` `0x1402a0850` presses DRS when more than 200 m from a corner and 5 s since last use) | controllers, `DRS::step` |
| `Car::accG` | car step (not traced here) | controllers (LATG, LONG), DRS `LIMIT_G` |
| `Car::getSpeed()` (`0x140272160`) | car body | controller SPEED_KMH |
| `ISuspension::getStatus()->travel` (slot +0x48) of wheels 2 and 3 | suspension step | controller SUS_TRAVEL_* |
| `Car::splineLocatorData.npos` | `Car::postStep` from `SplineLocator` (previous step) | `DRSManager::isDRSAvailable` |
| `Car::damageZoneLevel[0..3]` | `Car::onCollisionCallBack` (`0x140274650`), `Car::setDamageLevel` (`0x140275b20`). The callback keeps the maximum of (impact speed in km/h * `mechanicalDamageRate`) per zone. Zone = horizontal direction of the impact point in body axes (`body->worldToLocal`), normalised: `abs(z) > 0.707` -> 0 if z > 0 (FRONT) else 1 (REAR); otherwise 2 if x >= 0 (the `ZONE_LEFT_*` keys) else 3 (RIGHT). Index 4 gets every hit and is not used by the wings. | wing damage |
| `PhysicsEngine::mechanicalDamageRate` | default 1.0 (`PhysicsEngine::PhysicsEngine` `0x140262430`); offline `cfg/assists.ini [ASSISTS] DAMAGE * 0.01` (`DrivingAssistManager::DrivingAssistManager`); online from the server (`ACClient::handshakeTCP`) | switches wing damage on (must be > 0) and scales the damage levels |
| `PhysicsEngine::ambientTemperature` | 26 in the constructor, then session ini (`RaceManager::initOffline`) or server message (`ACClient::onMessageTCP`); no other writer found by member name | air density |
| `WingState::angle` or `inputAngle` | `SetupManager::initItems` (`0x140289570`) registers one setup item `WING_n` per wing, in degrees: it points at `inputAngle` when the wing has a controller, else at `angle` | wing angle |
| `PhysicsEngine::slipStreams` | every `Car` and every `NetCarStateProvider` (remote cars) registers one | slipstream |

### Outputs (who reads what this system writes)

| Output | Read by |
|---|---|
| Forces on `Car::body` | ODE (`core->step`), then everything that follows the body: suspension loads, tyre loads |
| `AeroMap::airDensity` | wings; copied to `CarPhysicsState::airDensity` by `Car::getPhysicsState` (`0x140270d70`); shared memory (`SharedMemoryWriter::updatePhysics` `0x140186ef0`); audio |
| `PhysicsEngine::getAirDensity()` (no slipstream) | also `Engine::step` (`0x1402880e0`), which scales engine torque by `rho * 0.82630974` |
| `AeroMap::getCurrentLiftKG` | `AIDriver::stepGasBrake` (`0x1402a08e0`), `Telemetry::step` (`0x1402bf5b0`) |
| `AeroMap::getCurrentDragKG` | `RaceEngineer::getAccelerationProfile` (`0x14027b550`), `Telemetry::step` |
| wing tables and angles | `RaceEngineer::projectWingsDrag` (`0x14027ccf0`) and `RaceEngineer::projectWingsLift` (`0x14027cdc0`): AI braking / grip estimates at a given speed |
| `WingState` vector | `Car::getWingState` -> car state (`CarAvatar::wingsStatus`), replay, the wings app, wing animation (`CarAnimations::update` `0x140062160` reads `wingsStatus[i].angle`, which is the modified angle) |
| `WingState::angle` / `inputAngle`, read straight from the physics `Wing` | `CarAvatar::getWingAngle` (`0x1400d3aa0`): returns `inputAngle` when the wing has a controller, else `angle`. Used by the pit-stop quick menu (`QuickMenuPitstop`), not by the animation |
| `DRS::isAvailable`, `isActive` | `Car::getPhysicsState` (flag values 2 and 4 in `statusBytes`, only when `isPresent`), `AIDriver::stepDRS` |
| `SCarStateAero` (CD, CL_Front, CL_Rear) | filled from `dynamicCD` / `dynamicCL`, so always 0 on wing cars |

### Place in the step order

`Car::step`: `Car::updateAirPressure` (air density) -> ... -> `Car::stepComponents`:
BrakeSystem, EDL, suspensions, `Tyre::step` x4, HeaveSpring x2, **`DRS::step`, `AeroMap::step`**,
Kers, ERS, steering, drivetrain, ... Then ODE integrates. Then `Car::postStep` moves the wake.

Link to the tyre: aero never calls the tyre. It only reads the four contact points the tyre wrote
earlier in the step, and its body forces reach the tyres one step later as load.

Remote cars in multiplayer have their own copy of the controller logic
(`NetCarStateProvider::initWings` `0x14011aa80`, `NetCarStateProvider::initDRS` `0x140119a60`,
`NetCarStateProvider::stepWings` `0x14011d7c0`) driven from the received `CarPhysicsState` instead
of a `Car`; that is display only.

---

## 7. Port notes

**Effort: S to M.** The maths is about 150 lines (two force functions, one controller filter, a
cone test, a plane distance). The work is in the loader and in matching float operation order.

Port before it:
- `Curve` (done) and the ini reader (done for tyres).
- A rigid body with the six `IRigidBody` calls used: `getLocalPointVelocity`, `worldToLocalNormal`,
  `localToWorld`, `addLocalForceAtLocalPos`, `getPosition`, `getVelocity`.
- Tyre contact points (done) for ground height and the wind plane.
- For the full feature set: car controls, `accG`, speed, suspension travel, `damageZoneLevel`,
  spline position (DRS zones). All of these are plain inputs and can be stubbed.

Suggested order inside the port: (1) loader, (2) `Wing::addDrag` / `addLift` as pure functions,
(3) `Wing::step` geometry, (4) controllers, (5) DRS, (6) air density + slipstream, (7) wind.

Bit-exact test with an oracle (same idea as the tyre oracle):
- **Level 0, let the game load the car.** `tools/tyre_oracle` already maps acs.exe in-process and
  runs the game's `Tyre::init` against a fake car. The same trick should work for
  `AeroMap::init` (`0x1402b5ca0`): it needs a `Car` block with `body`, `unixName` and whatever
  `Car::getConfigPath` reads, plus a `suspensions` vector whose entries 0 and 2 answer slot +0x50
  (`getBasePosition`, used by `RaceEngineer::getPointFrontShare`). That gives real `Wing` objects
  with their `Curve`s filled by the game's own loader, and a reference dump of `WingData` to check
  the Rust loader against. Not tried; `DRS::init` (`0x1402b4330`) can follow the same way.
- **Level 1, pure force functions.** Call the game's `Wing::addDrag` (`0x1402b2420`) and
  `Wing::addLift` (`0x1402b2730`) on those wings. Needs faking: `Car::body` (a fake `IRigidBody`
  vtable whose slot +0xf0 records force and position), `Car::ksPhysics` pointing at a block with
  `mechanicalDamageRate`, plus `aeroMap.airDensity` and `damageZoneLevel[5]` in the `Car` block.
  Set `status.aoa`, `yawAngle`, `angle`, `angleMult`, `groundHeight` by hand. Sweep v, angle,
  height, damage. Compare `cd`, `cl`, `dragKG`, `liftKG`, `liftVector`, recorded force.
- **Level 2, `Wing::step`** (`0x1402b2bc0`). Fake body also answers slots +0x100, +0x68, +0x50; the
  fake `Car` needs four `Tyre` blocks with `contactPoint` / `unmodifiedContactPoint` set, and
  `ksPhysics->wind.vector`. This covers angle of attack, yaw, ground height and wind in one call.
- **Level 3, controllers and DRS.** `DynamicWingController::step` (`0x1402aaa70`) with a fake `Car`
  (`controls`, `accG`, body for `getSpeed`); `DRS::step` (`0x1402b4e60`) with a fake
  `ksPhysics->track->drsMamanger` holding a zone list and `splineLocatorData.npos`.
- **Level 4, standalone functions.** `SlipStream::setPosition` (`0x1402aae80`) and
  `SlipStream::getSlipEffect` (`0x1402aac60`) need nothing but a `SlipStream` block;
  `PhysicsEngine::getAirDensity` and `PhysicsEngine::stepWind` need a `PhysicsEngine` block with a few floats;
  `Car::updateAirPressure` (`0x140276ae0`) needs the fake body plus a `slipStreams` vector.
- Library calls to bind in the oracle: `atanf`, `sinf` (wings), double `sin` and `_fdtest`
  (`stepWind`), `printf` (loaders, empty-curve warning).

Things to keep identical:
- `atanf`, `sinf` come from MSVCR120; the tyre port already had to match library functions.
  `stepWind` uses double `sin` then narrows to float.
- `aoa` is `atanf(v.y * (1/v.z))`, a multiply by a reciprocal, not a divide. Degree constants are
  57.29578 (0x42652ee1, the nearest float to 180/pi) and 0.017453 (0x3c8ef998, a truncated decimal:
  the nearest float to pi/180 would be 0x3c8efa35).
- The `x^2` in the damage term is a small integer-power loop; `(1/300) * level` first, then squared.
- Keep the `* 0` terms of the cross products if negative zero or NaN propagation must match.
- The controller filter uses the literal 0.003, not the step size.
- Wings are stepped and their forces added in file order (wings, then fins); ODE sums forces, so the
  order of the float additions into the body matters for exact equality.

---

## 8. Open questions

1. **DRS race rules.** `DRSManager::detections` and `DRSDetectionStatus` exist, and
   `DRSManager::wasRaceSwitchedOn` (`0x140279510`) reads them from
   `PhysicsAvatar::onPhysicsStepCompleted` (`0x1401221a0`), but no function that fills them was found.
   `Track::step` calls only an empty merged function on the manager. So in this build the physics
   allows DRS purely by zone position; the detection point and the gap to the car ahead look unused.
   Whether the UI or the server enforces them elsewhere was not traced.
2. **Wind sign.** The wind vector is added to the car velocity, which only makes sense if
   `wind.vector` points to where the wind comes from. Not checked against the on-screen wind arrow.
3. **Left or right.** Answered from the code: `Car::onCollisionCallBack` puts an impact whose
   body-frame x is >= 0 into zone 2, and the loader fills zone 2 from `ZONE_LEFT_*`, so +x is the
   car's left as far as the key names can be trusted. The same callback backs this up (review): a
   hit with zones 0 and 2 both damaged calls slot +0x78 on `suspensions[0]`, zones 0 and 3 on
   `suspensions[1]`, zones 1 and 2 on `suspensions[2]`, zones 1 and 3 on `suspensions[3]`, so zone 2
   is the side of wheels 0 and 2. That wheels 0 and 2 are the left ones is taken from the usual
   wheel order (LF, RF, LR, RR), not re-derived here.
4. **`Car::postStep` registration.** Answered: `Car::Car` builds the lambda whose call wrapper is
   0x14026ef00 and adds it to `ksPhysics->evOnStepCompleted` (read in the pseudo-C of `Car::Car`,
   right after `SlipStream::init`).
5. **Changing temperature.** `RaceManager::initOffline` builds a random temperature curve and hands
   it to `PhysicsEngine::setDynamicTempData` (`0x140141210`), but a search of all of `re/decomp` by
   member name finds no reader of `dynamicTemp` and only three writers of `ambientTemperature`
   (constructor, `initOffline`, `ACClient::onMessageTCP`). So air density looks constant over an
   offline session apart from slipstream. An access by raw offset would not show up in that search.
6. **`getPosition(0)` argument.** `IRigidBody::getPosition` takes a float that the ODE version
   ignores (interpolation time, my guess).
7. **`plane4f` constructor.** Answered, see 5.4 (`plane4f::plane4f` `0x140117ed0`). The exact order
   of its float operations still has to be copied from the pseudo-C when porting.
8. **`INIReader::getFloat` on a missing key** returns 0.0 on the path I read (empty string); the
   variant with `verbose` set was not followed. A value that is present but not a number throws
   ("invalid stof argument"). What `INIReader::getString` itself reports for a missing key when
   `crashAtError` is on was not read.
9. **Old `[DATA]` path** was mapped from the code only; no car data exists to try it on. I would not
   port it until a car needs it.
10. **`WingState::frontShare`** is computed once at load by `RaceEngineer::getPointFrontShare` and
    appears to be display only; no physics reader was found (not searched exhaustively).
11. **Operation order.** Formulas are transcribed from the pseudo-C. Constants were checked in the
    disassembly for `Wing::step`, `Wing::addLift`, `DynamicWingController::step`, `initCommon`,
    `stepWind`, `updateAirPressure`, `getAirDensity`, and in review for `Wing::addDrag` (sum of
    squares is `(x*x + y*y) + z*z`; `D = q * cd * airDensity * area * 0.5`) and the wind lambda. The
    two old-format functions, `SlipStream::setPosition`, `getPointGroundHeight` and
    `getGroundWindVector` were read in the pseudo-C only.
12. **Choice made:** the brief asked for "ride-height maps" and "body drag" as separate items. In
    the code they are not separate systems (a per-wing height multiplier, and a wing named BODY), so
    they are described inside 5.4 instead of in sections of their own.
13. **Slipstream assist online.** The `[ASSISTS] SLIPSTREAM` block in `DrivingAssistManager` runs
    only without a client. Whether a server setting reaches `slipStream.speedFactor` some other way
    was not traced (no other writer of `speedFactor` was found by member name). Which car index 0
    is (assumed: the player's car) was not traced. Which `cfg/assists.ini` is opened is answered
    (review): the reader is an `INIReaderDocuments`, so it is the copy in the user's documents
    folder; how that class falls back when the file is missing there was not read.
14. **Empty `LUT_GH_*` value.** With an empty value the path handed to `Path::fileExists`
    (`0x1402305e0`) is the car's data folder itself. That function first asks the `.acd` archive
    (`FolderEncrypter::fileExists`) for the file name, then tries to open the path as a file stream.
    The curve stays empty on the assumption that both fail for an empty name / a folder; the archive
    lookup was not read.
15. **Review note.** A second pass re-read every function of the six source folders against this
    text. Corrected then: unknown controller `INPUT` is not a crash with the installed config, the
    degenerate case of `getPointGroundHeight`, the role of `CarAvatar::getWingAngle`, the meaning of
    the "Cars" column in 4.2, and the slipstream assist setting that was missing.
16. **Second review note.** An independent pass checked about 60 statements against the pseudo-C,
    the disassembly, `re/types` and the car data. Wrong and now corrected: the sign of `aoa`
    (positive is nose-down, air from above, 5.4), the fin table value quoted without its car
    (5.4 notes), and which `cfg/assists.ini` is read (4.4, question 13). Added: the override
    save / restore lines of `Wing::step`, the 0.0 start value of a controller's `outputAngle`, the
    `DRS::init` loop that does not stop at a gap, `Car::getConfigPath`, the online weather message,
    and the thread-pool remark in 5.2. Still not verified by anyone: the lower clamp bound of the
    online wind speed, `INIReaderDocuments`' fallback, and the `.acd` lookup of question 14.
