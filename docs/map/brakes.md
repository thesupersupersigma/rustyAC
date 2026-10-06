# Brakes

Source: `acs.exe` + `acs.pdb`, read through the local index only (`re/decomp/` pseudo-C, `re/types/`
struct layouts, `tools/re_query.py`, `tools/disasm.py` for exact operation order and constants,
`tools/ini_survey.py` over the 113 cars in `cardata/`, and the commented Kunos example car in the SDK).
Raw material: `re/car/brakes/decomp/` (pseudo-C copies), `re/car/brakes/asm/` (disassembly of
`BrakeSystem::step`, `stepTemps`, `loadINI`, `DynamicController::eval`, `lagToLerpDeltaK`),
`re/car/brakes/*.txt` (struct layouts). Nothing in the game folder, the Ghidra project or git was
changed. No Rust was written.

Wheel order used everywhere below: `tyres[0]` = left front, `[1]` = right front, `[2]` = left rear,
`[3]` = right rear (confirmed by `DynamicController::getInput`: `LOAD_SPREAD_LF` uses `tyres[0]`,
`SUS_TRAVEL_LR` uses `suspensions[2]`).

---

## 1. Plain-English summary

Each physics step (333 times a second) the brake system turns the brake pedal into a braking
torque for each of the four wheels.
It takes the pedal position (0 to 1), multiplies it by the car's maximum brake torque and by the
"brake power" setup percentage, and splits the result between the front and the rear wheels
according to the brake bias.
The bias normally comes from the car data or the setup and can be nudged from the cockpit; a few
road cars instead let a small programmable controller pick the bias every step from things like
how the load is spread across the front wheels (electronic brake balance).
The handbrake adds its own torque on the two rear wheels only.
A few cars also brake one rear wheel on their own when the driver steers, to help the car turn
(steer-brake).
Cars with brake temperature data (22 of 113) have a temperature per disc: the disc heats up in
proportion to braking torque times wheel speed, cools toward the air temperature faster the
faster the car goes, and a curve turns the temperature into a strength factor, so cold or
overheated brakes are weaker (fade).
The brake system does not slow the wheel itself: it only writes the torques into each tyre's input
slots. The tyre then subtracts the torque from the wheel's spin, always against the direction of
rotation, and decides when a wheel is locked.
ABS does not change the torque numbers: it sets a separate on/off factor per wheel that the tyre
multiplies in, so the brake is fully released on a wheel while that factor is zero.
The pit speed limiter, a "controls locked" state, a gentle-stop flag and the AI can all force
the brakes on without the driver.

---

## 2. Classes

| Class | Size | What it is for |
|---|---|---|
| `BrakeSystem` | 0x3f0 | The whole brake system of one car (`Car::brakeSystem`, at `Car`+0x2ca0). Holds bias, max torque, handbrake torque, EBB state, the four discs, the steer-brake, bias limits. Does the per-step work. |
| `BrakeDisc` | 0x90 | One disc: temperature `t`, `coolTransfer`, `torqueK`, `coolSpeedFactor`, `perfCurve` (`Curve`). `BrakeSystem::discs[4]`, same order as the tyres. |
| `SteerBrake` | 0x30 | `isActive` flag + one `DynamicController`. Torque added to one rear wheel. |
| `DynamicController` | 0x28 | Small programmable formula loaded from a `ctrl_*.ini` file: a list of stages, each "take a car signal, pass it through a lookup table, low-pass filter it, add or multiply into the running result, clamp". Shared class (`eval` is also called by the drivetrain, the turbos, ERS, KERS, the steering system and the anti-roll bars). Members: `car`, `stages` (vector), `ready`. |
| `DynamicControllerStage` | 0xa0 | One stage: `inputVar`, `combinatorMode`, `lut` (`Curve`), `filter`, `upLimit`, `downLimit`, `currentValue`, `constValue`. |
| `EBBMode` (enum) | 4 | `Disabled` = 0, `Internal` = 1, `DynamicController` = 2 (values from the compare chain in `BrakeSystem::step`). |
| `TyreInputs` | 0xc | Per wheel: `brakeTorque`, `handBrakeTorque`, `electricTorque`. This is the brake system's output (`Tyre::inputs`, at `Tyre`+0x0). |
| `Tyre::absOverride` | float | Per wheel 0..1 factor on `brakeTorque` (at `Tyre`+0x41c). Written by ABS and by the AI, read by the tyre and drivetrain. |
| `ABS` | 0xa8 | Anti-lock system (`Car::abs`). Belongs to the electronics map; here only its interface (it writes `absOverride`). |
| `EDL` | 0x38 | Electronic diff lock (`Car::edl`). Belongs to the electronics/drivetrain map; listed because it adds to `inputs.brakeTorque`. |

Not physics (listed only): `BrakeDiscGraphics` (disc glow, reads `[DISCS_GRAPHICS]`),
`BrakeBiasNotifier` (on-screen message when the bias changes), `CarBrakeLights`,
`AIBrakeObstacle` / `AIBrakePointData` / `AIBrakeProfiler` (AI braking points, AI map),
`CarHintLoader::CarHintLoader` 0x140068860 (second reader of brakes.ini: only `[DATA]
COCKPIT_ADJUSTABLE`, for the loading-screen hint text).

---

## 3. Key functions

### Loading / init

```
Car::Car                                   0x14026bf00
├─ BrakeSystem::BrakeSystem                0x14026be20   constructor defaults (in Car.obj)
│  ├─ BrakeDisc::BrakeDisc x4              0x14026bde0   t=0, coolTransfer=0.005, torqueK=0.1, coolSpeedFactor=0.001
│  └─ other defaults: frontBias=0.7, brakePowerMultiplier=1, electronicOverride=1 (zeroed by init),
│       handBrakeTorque=0, ebbInstant=0.5, limitDown=0, limitUp=1, rearCorrectionTorque=0, biasOverride=-1,
│       hasCockpitBias=true, biasStep=0.005, ebbMode=Disabled, ebbFrontMultiplier=1.1,
│       steerBrake.isActive=false, hasBrakeTempsData=false (brakePower is not set here)
└─ BrakeSystem::init(car)                  0x14028d690   right after Car::initCarData, before the suspensions are built
   ├─ sets car, brakePower=2000, frontBias=0.7, brakePowerMultiplier=1, electronicOverride=0
   ├─ BrakeSystem::loadINI("content/cars/<car>/data/")   0x14028d870
   │  ├─ Car::getConfigPath                0x140270420   ("data_<configName>/" variant of the file if the car has a config name)
   │  ├─ INIReader on brakes.ini: [DATA], [EBB], [TEMPS_FRONT], [TEMPS_REAR]
   │  │    (INIReader::getFloat 0x1402358c0, INIReader::getInt 0x140235c70,
   │  │     INIReader::hasSection 0x1402370b0, INIReader::getCurve 0x140235040)
   │  │    (order in the code: [DATA], [EBB], the steer-brake file, the [TEMPS_*] loop, setup.ini)
   │  ├─ Path::fileExists(steer_brake_controller.ini)  0x1402305e0
   │  │    └─ DynamicController::DynamicController(car, path)  0x1402af330  -> steerBrake.controller, isActive = true
   │  └─ INIReader on setup.ini: [FRONT_BIAS] MIN / MAX -> limitDown / limitUp
   └─ Path::fileExists(ctrl_ebb.ini)
        └─ DynamicController::DynamicController(car, path)     -> ebbController, ebbMode = DynamicController
             └─ lagToLerpDeltaK(FILTER, 0.004, 0.003)  0x14005d7c0   per stage

SetupManager::initItems                    0x140289570   registers setup items BRAKE_POWER_MULT -> brakePowerMultiplier (x0.01)
                                                         and FRONT_BIAS -> frontBias (x0.01)
Car::forcePosition                         0x14026fe10   (car teleported / returned to pits)
└─ BrakeSystem::reset                      0x14028e490   biasOverride = -1, every disc t = ambient temperature
```

### Every physics step

```
Car::step                                  0x140275da0
├─ Car::pollControls                       0x140274e70   fills Car::controls (brake, handBrake) from the controls provider;
│                                                        an AI provider also writes Tyre::absOverride (AIDriver::stepSuperhuman 0x1402a2a60)
├─ control overrides: locked controls -> brake = 1; isGentleStopping -> brake = 0.2
└─ Car::stepComponents                     0x1402764d0
   ├─ BrakeSystem::step(dt)                0x14028e640   FIRST component of the step
   │  ├─ DynamicController::eval           0x1402b0c00   only if ebbMode == DynamicController (bias)
   │  │   └─ DynamicController::getInput   0x1402b0d70   + Curve::getValue 0x140206990, per stage
   │  ├─ Car::getSpeed                     0x140272160   only if ebbMode == Internal
   │  ├─ writes tyres[0..3].inputs.brakeTorque and tyres[2..3].inputs.handBrakeTorque
   │  ├─ DynamicController::eval           only if steerBrake.isActive (extra torque on one rear wheel)
   │  ├─ BrakeSystem::stepTemps(dt)        0x14028e920   only if hasBrakeTempsData and tyres[0].aiMult <= 1
   │  │   └─ Curve::getValue (perfCurve) + Car::getSpeed, per disc
   │  ├─ BrakeSystem::saveTempsRunFame     0x14028e4e0   developer recording, only while tempRunFile is good
   │  └─ electronicOverride = 0
   ├─ EDL::step                            0x1402bb460   may add torque to the faster wheel of one axle (inputs.brakeTorque);
   │                                                     the pair is tyres 2/3, or 0/1 when tractionType is FWD or AWD (EDL::init 0x1402babf0)
   ├─ suspensions, then Tyre::step x4      0x140283800   consumes inputs.* and absOverride (section 5.5)
   │   └─ Tyre::updateAngularSpeed 0x140284fa0 / Tyre::updateLockedState 0x140285070
   ├─ ... Kers::step 0x1402b7e10, ERS::step 0x1402930e0   read brakeTorque*absOverride; ERS writes rearCorrectionTorque, electricTorque
   ├─ ... Drivetrain::step  (Drivetrain::step2WD 0x1402694e0 reads the brake torques to release locked driven wheels;
   │                         step4WD 0x14026a220 / step4WD_new 0x14026ad80 do not read them, see section 5.5)
   ├─ ABS::step                            0x14028f610   writes Tyre::absOverride (used by the NEXT step)
   ├─ SpeedLimiter::step                   0x1402bb910   writes brakeSystem.electronicOverride (used by the NEXT step)
   └─ SetupManager::step                   0x14028d090   per-item loop 0x140288b20: for an attached item, writes
                                                         multiplier * newValue into frontBias / brakePowerMultiplier when it differs
```

Small accessors: `BrakeSystem::getFrontBias` 0x14028d630 (returns `biasOverride`, or `frontBias`
when the override is -1), `BrakeSystem::getBrakePower` 0x14028d620 (`brakePower`),
`BrakeSystem::isUsingEBB` 0x1402befa0 (`ebbMode != Disabled`),
`BrakeSystem::setManualFrontBias(int clicks)` 0x14028e5d0 (cockpit adjust),
`BrakeSystem::activateTempRunFile(bool)` 0x14028d400 (opens `TempRuns/<car>@<track>.trun`, called
from two lambdas of the `ACSuspension` developer app),
`BrakeSystem::saveTempsRunFame` 0x14028e4e0 (appends 9 raw floats per step to that file: the car
speed in m/s, the four `status.angularVelocity`, the four `inputs.brakeTorque`; it changes nothing).

---

## 4. Data it reads

### brakes.ini (113 of 113 cars) - read by `BrakeSystem::loadINI`

A missing key gives 0: `INIReader::getFloat` returns 0.0 for an absent or empty value and
`loadINI` stores the result unconditionally, so the 2000 / 0.7 written by `init` never survive.

| [SECTION] KEY | Member | Conversion | Units | Cars |
|---|---|---|---|---|
| `[HEADER] VERSION` | not read by the physics | - | - | 113 (1 in 109 cars, 2 in 4) |
| `[DATA] MAX_TORQUE` | `brakePower` | as is | Nm; torque at full pedal for one front wheel plus one rear wheel (each front wheel gets `MAX_TORQUE*bias`, each rear `MAX_TORQUE*(1-bias)`, so the four wheels together get `2*MAX_TORQUE`) | 113 |
| `[DATA] FRONT_SHARE` | `frontBias` | as is | fraction 0..1 to the front | 113 |
| `[DATA] HANDBRAKE_TORQUE` | `handBrakeTorque` | as is | Nm per rear wheel at full handbrake | 113 (0 in 30 cars) |
| `[DATA] COCKPIT_ADJUSTABLE` | `hasCockpitBias` | `!= 0` | bool | 113 (1 in 42 cars) |
| `[DATA] ADJUST_STEP` | `biasStep` | `* 0.01` | percent per click -> fraction | 113 (0.5 in 111) |
| `[EBB]` (section present) | `ebbMode = Internal` | - | - | 0 of 113 |
| `[EBB] FRONT_SHARE_MULTIPLIER` | `ebbFrontMultiplier` | used only if `>= 1.1`, otherwise 1.1 | factor on the front load share | 0 of 113 |
| `[TEMPS_FRONT]` / `[TEMPS_REAR]` (section present) | `hasBrakeTempsData` (see quirk below) | - | - | 22 of 113 (always both) |
| `TORQUE_K` | `discs[i].torqueK` | as is | degC per kJ of brake work (heat = power[W] * K * 0.001 * dt) | 22 |
| `PERF_CURVE` | `discs[i].perfCurve` | inline `(|T=mult|...|)` list or a .lut file name | x = disc degC, y = torque multiplier | 22 |
| `COOL_TRANSFER` | `discs[i].coolTransfer` | as is | 1/s at standstill | 22 |
| `COOL_SPEED_FACTOR` | `discs[i].coolSpeedFactor` | as is | extra cooling per km/h | 22 |
| `[DISCS_GRAPHICS] ...` | not read by the physics (only `BrakeDiscGraphics::BrakeDiscGraphics` 0x14005ca40) | - | - | 108 |

`[TEMPS_FRONT]` fills `discs[0]` and `discs[1]`, `[TEMPS_REAR]` fills `discs[2]` and `discs[3]`
(the same four keys are read twice, once per disc).

Quirk (confirmed in the code): the loop sets `hasBrakeTempsData = true` when the section exists
and `= false` when it does not, for the front section first and the rear section second. The
final value therefore depends only on `[TEMPS_REAR]`. A car with only `[TEMPS_FRONT]` gets no
temperature model; a car with only `[TEMPS_REAR]` gets it with empty front curves, and an empty
`Curve` returns 0, so the front brakes would produce no torque. No car in `cardata/` does either.

Keys present in car data but never read by acs.exe (no such string in `BrakeSystem::loadINI`):
`[TEMPS_*] TRANSFER` (5 cars, three of them Kunos cars), and from mod cars `[STEER_BRAKE]`,
`[_EXTENSION]`, `COOL_SPEED_FACTOR_0`, `CORE_TRANSFER_*`, `DEFAULT_BLANKING`. `[HEADER] VERSION=2` (4 cars) changes nothing in acs.exe: there
is no version switch in `BrakeSystem`.

### setup.ini - read by `BrakeSystem::loadINI` (limits) and `SetupManager::initItems` (items)

| [SECTION] KEY | Member | Conversion | Default when missing | Cars |
|---|---|---|---|---|
| `[FRONT_BIAS] MIN` | `limitDown` | `* 0.01` | 0.0 (constructor) | 45 of 113 |
| `[FRONT_BIAS] MAX` | `limitUp` | `* 0.01` | 1.0 (constructor) | 45 of 113 |
| setup item `FRONT_BIAS` | `frontBias` | setup value `* 0.01` (`SetupItem::multiplier`) | car's `FRONT_SHARE` | 45 |
| setup item `BRAKE_POWER_MULT` | `brakePowerMultiplier` | setup value `* 0.01` | 1.0 | 113 (range 80..100 in 111 cars) |

The limits are only read when setup.ini opens and has a `[FRONT_BIAS]` section. They clamp the
bias every step (also the EBB result) and clamp the cockpit adjustment.

### ctrl_ebb.ini (10 of 113 cars) and steer_brake_controller.ini (3 of 113 cars)

Both are `DynamicController` files. If `ctrl_ebb.ini` exists, `ebbMode = DynamicController` (this
wins over `[EBB]` because it is set after `loadINI`). If `steer_brake_controller.ini` exists,
`steerBrake.isActive = true`. Cars: EBB - alfa_romeo_giulietta_qv (+_le), ferrari_laferrari,
ks_alfa_giulia_qv, ks_ferrari_812_superfast, ks_lamborghini_aventador_sv,
ks_lamborghini_huracan_performante, ks_lamborghini_sesto_elemento, ks_porsche_panamera,
vrc_formula_alpha_2026_csp. Steer-brake - ks_mclaren_p1, ks_porsche_panamera, mclaren_mp412c.

Sections `[CONTROLLER_0]`, `[CONTROLLER_1]`, ... are read until the first missing index:

| KEY | Stage member | Notes |
|---|---|---|
| `INPUT` | `inputVar` | one of `BRAKE GAS STEER LATG LONG SPEED_KMH GEAR SLIPRATIO_MAX SLIPRATIO_AVG SLIPANGLE_FRONT_AVG SLIPANGLE_FRONT_MAX SLIPANGLE_REAR_AVG SLIPANGLE_REAR_MAX OVERSTEER_FACTOR REAR_SPEED_RATIO STEER_DEG CONST RPMS WHEEL_STEER_DEG LOAD_SPREAD_LF LOAD_SPREAD_RF AVG_TRAVEL_REAR SUS_TRAVEL_LR SUS_TRAVEL_RR`. An unknown name prints `ERROR: INPUT UNDEFINED ...` and is a critical error only when `INIReader::crashAtError` is set; otherwise loading goes on with `inputVar = UndefinedInput` (0): the stage is still pushed and `ready` is still set (read to the end of the constructor), `getInput` returns 0.0 for it, so the stage value is the constant `lut.getValue(0.0)`. `CONST` has enum value 0x11. |
| `COMBINATOR` | `combinatorMode` | `ADD` (`eAdd`), `MULT` (`eMult`), anything else stays `eUndefinedMode` and prints a load error. Numeric values 1 / 2 / 0 are read from the compare chain in `eval`. |
| `LUT` | `lut` | curve (inline list or file), not read for `CONST` |
| `CONST_VALUE` | `constValue` | only for `INPUT=CONST` |
| `FILTER` | `filter` | stored as `lagToLerpDeltaK(FILTER, 0.004, 0.003)` = `((1/0.003) * 0.004) * (1 - FILTER) * (1/0.003)` |
| `UP_LIMIT` | `upLimit` | clamp of the running result after this stage |
| `DOWN_LIMIT` | `downLimit` | same; the clamp is skipped when both limits are exactly 0 |

Two things about these files that a port must copy (both confirmed in the code):

- **Curve points are not sorted.** `INIReader::getCurve` adds the `x=y` pairs in file order and
  `Curve::addValue` 0x140205ae0 only appends; `Curve::getValue` then walks the x values from the
  start and stops at the first one that is `>= x`, interpolating with the point before it. Two
  Kunos EBB files have x values out of order: ferrari_laferrari
  `(|0=0.68|0.3=0.64|0.1=0.60|0.5=0.57|0.1=0.60|0.3=0.64|1=0.68|)` and ks_ferrari_812_superfast
  `(|0=0.66|0.4=0.63|0.1=0.62|0.5=0.61|0.1=0.62|0.4=0.63|1=0.66|)`. Worked through `getValue` by
  hand, the LaFerrari table gives 0.68..0.64 for a load spread of 0..0.3, then jumps to about
  0.585 and falls to 0.57 at 0.5, then jumps to about 0.651 and rises to 0.68 at 1.0 (not the
  symmetric V the author probably meant; the stage's `FILTER=0.95` smooths the jumps over time).
  Sorting the points would change the result.
- **`ready` is never tested.** Neither `BrakeSystem::step` nor `DynamicController::eval` looks at
  `DynamicController::ready`; a controller with no stages simply returns 0.0.

### Other files that reach the brakes (owned by other maps)

- `ers.ini [KINETIC] BRAKE_REAR_CORRECTION` (5 cars) -> `ERS::rearCorrectionTorque` -> each step
  `BrakeSystem::rearCorrectionTorque` (`ERS::init` 0x140291c30, `ERS::step`).
- `electronics.ini [ABS]` / `[ABS_V2]` (`SLIP_RATIO_LIMIT`, `PRESENT`, `ACTIVE`, `RATE_HZ`,
  `CHANNELS`, `CURVE`) -> `ABS` (electronics map).
- `kers.ini [KERS] BRAKE_LEVEL` -> `Kers::brakeForMaxCharge = brakePower * BRAKE_LEVEL * 2.0`
  (`Kers::init` 0x1402b7360, the only use of `getBrakePower` at load time).
- `ai.ini [BRAKE_BIAS] VALUE` (0 of 113 cars; AI cars only) -> `AIDriver::AIDriver` 0x140298c60
  marks the `FRONT_BIAS` setup item as attached and sets its `newValue = VALUE` (a percentage), so
  `frontBias` becomes `0.01 * VALUE` at the next `SetupManager::step`.

---

## 5. Each step: inputs, maths, outputs

All arithmetic is single-precision float. `dt` = 0.003 s. Operation order below is taken from the
disassembly (`re/car/brakes/asm/`).

### 5.1 `BrakeSystem::step(dt)`

Inputs read: `car->controls.brake`, `car->controls.handBrake`, `frontBias`, `biasOverride`,
`ebbMode`, `ebbFrontMultiplier`, `limitDown`, `limitUp`, `electronicOverride`, `brakePower`,
`brakePowerMultiplier`, `rearCorrectionTorque`, `handBrakeTorque`, `steerBrake`,
`hasBrakeTempsData`, `car->tyres[i].status.load`, `car->tyres[0].aiMult`, `Car::getSpeed()`
(= `car->valueCache.speed`, m/s, cached by `Car::stepPreCacheValues` 0x1402768c0).

```
bias = (biasOverride != -1.0) ? biasOverride : frontBias

if ebbMode == Internal (1):
    front = load[1] + load[0]
    total = load[3] + load[2] + front
    if total != 0 and speed * 3.6 > 10.0:
        ebbInstant = clamp(front / total * ebbFrontMultiplier, 0, 1)
    else:
        ebbInstant = frontBias                 # note: frontBias, not biasOverride
    bias = ebbInstant
elif ebbMode == DynamicController (2):
    bias = ebbController.eval()                # ebbInstant is NOT updated in this mode

if bias > limitUp: bias = limitUp
elif bias < limitDown: bias = limitDown

pedal = (electronicOverride > controls.brake) ? electronicOverride : controls.brake
torque = (brakePower * brakePowerMultiplier) * pedal

tyres[0].inputs.brakeTorque = tyres[1].inputs.brakeTorque = torque * bias
rear = (1.0 - bias) * torque - rearCorrectionTorque
if rear < 0: rear = 0
tyres[2].inputs.brakeTorque = tyres[3].inputs.brakeTorque = rear

tyres[2].inputs.handBrakeTorque = tyres[3].inputs.handBrakeTorque = controls.handBrake * handBrakeTorque
                                              # front handBrakeTorque is never written: stays 0 from Tyre::Tyre

if steerBrake.isActive:
    v = steerBrake.controller.eval()
    if v >= 0: tyres[3].inputs.brakeTorque += v     # right rear
    else:      tyres[2].inputs.brakeTorque -= v     # left rear gets |v|

if hasBrakeTempsData and tyres[0].aiMult <= 1.0: stepTemps(dt)
if tempRunFile is in a good state: saveTempsRunFame()
electronicOverride = 0
```

Points that follow from this:

- The cockpit bias (`biasOverride`) is ignored whenever EBB is active (either mode), because the
  EBB result replaces `bias`.
- `electronicOverride` is a one-step request: whoever wants automatic braking must write it every
  step. It is a pedal value, combined with the driver's pedal by taking the larger. Nothing in
  `BrakeSystem::step` clamps the pedal or the torque to a maximum: the only writer,
  `SpeedLimiter::step`, stores `RaceEngineer::getOptimalBrake()` unclamped, which is
  `min(front, rear)` of `(sum over the axle of grip_i * load_i) * 0.5 * loadedRadius / (brakePower * axle share)`
  with `grip_i = Tyre::getCorrectedD(Tyre::getDX(load_i))`, axle share = `frontBias` or
  `1 - frontBias` (the raw member: no `biasOverride`, no EBB, no `brakePowerMultiplier`), and the
  radius of `tyres[0]` / `tyres[2]`. That it can exceed 1 on a car with weak brakes is my reading
  of the formula, not something observed.
- `rearCorrectionTorque` is subtracted from each rear wheel's torque (hybrid cars: the rear
  axle already brakes through the electric motor). It is in Nm.
- The steer-brake output is in Nm and is added on top, not scaled by the pedal.
- Brake temperatures are skipped while `tyres[0].aiMult > 1` (the AI writes `aiMult` = 1.001 or
  its `ultraGrip` value on all four tyres in `AIDriver::stepGasBrake`; only tyre 0 is tested).
  `ultraGrip` is 1.2: `AIDriver::AIDriver` calls `AIDriver::setUltraGrip(1.2)` 0x14029f8c0 and
  nothing else calls it (the constructor does not read ai.ini `[ULTRA_GRIP]`; the only function
  that uses that string is a lambda of the `FormAI` developer app, which writes the key). So both
  values the AI writes are above 1 and an AI-driven car never runs `stepTemps`: no fade, disc
  temperatures frozen. The only other writer of `aiMult` is `Tyre::Tyre` (1.0), so nothing puts
  it back to 1 once an AI has driven the car.

### 5.2 `BrakeSystem::stepTemps(dt)` - heating, cooling, fade

For disc `i` = 0..3 (disc `i` belongs to `tyres[i]`), in this order:

```
T = tyres[i].inputs.brakeTorque = perfCurve_i.getValue(t_i) * tyres[i].inputs.brakeTorque   # fade, written back
cool = ((speed * 3.6) * coolSpeedFactor_i + 1.0) * coolTransfer_i
t1   = ((ambient - t_i) * cool) * dt + t_i
t_i  = ((|omega_i| * (T * torqueK_i)) * 0.001) * dt + t1
```

- `ambient` = `car->ksPhysics->ambientTemperature` (degC), `speed` = `Car::getSpeed()` in m/s,
  `omega_i` = `tyres[i].status.angularVelocity` in rad/s (the value left by the previous step,
  because `Tyre::step` runs later).
- `Curve::getValue` is piecewise linear, clamped to the first/last value outside the table, and
  returns 0 for an empty curve.
- The fade factor uses the temperature from before this step's update.
- Heating uses the already-faded torque and ignores `absOverride`, the handbrake and the EDL
  torque (EDL is added after `BrakeSystem::step`). The steer-brake torque is included.
- There is no upper limit, no heat exchange between discs, tyres or rims, and no separate pad or
  core temperature: one number per disc.
- Disc temperature starts at 0.0 (constructor) and is set to the ambient temperature by
  `BrakeSystem::reset` (called from `Car::forcePosition`).

Example of a curve (lotus_49): `(|0=0.70|300=0.9|500=1.0|600=1.0|800=0.7|1200=0.2|)` - 70 % torque
when cold, full torque between 500 and 600 degC, falling to 20 % at 1200 degC.

### 5.3 `DynamicController::eval()` - used for EBB bias and steer-brake torque

```
out = 0
for each stage, in file order:
    v = (inputVar == CONST) ? constValue : lut.getValue(getInput(inputVar))
    prev = currentValue
    if |v - prev| >= 0.001:
        k = clamp(filter * 0.003, 0, 1)          # 0.003 is hard-coded, not the dt argument
        v = (v - prev) * k + prev
    currentValue = v
    combinatorMode 0: out = 0;  ADD (1): out = out + currentValue;  MULT (2): out = out * v
    if not (downLimit == 0 and upLimit == 0):
        if out > upLimit: out = upLimit
        elif out < downLimit: out = downLimit
return out
```

With the stored `filter`, `k = clamp(1.3333 * (1 - FILTER), 0, 1)`: `FILTER=0.95` moves 6.7 % of
the way to the new value per step, `FILTER=0` (and anything below 0.25) follows instantly.

Inputs used by the brake controllers found in `cardata/` (`DynamicController::getInput`):
`LOAD_SPREAD_LF` = `load[0] / (load[0] + load[1])`; `SLIPANGLE_REAR_MAX` =
`max(|slipAngleRAD[2]|, |slipAngleRAD[3]|) * 57.29578`; `OVERSTEER_FACTOR` =
`((|sa[3]|+|sa[2]|)*0.5 - (|sa[0]|+|sa[1]|)*0.5) * 57.29578`; `BRAKE` = `controls.brake`;
`SPEED_KMH` = `getSpeed() * 3.6`; `STEER_DEG` = `car->steerLock * controls.steer`; `GEAR` =
`drivetrain.currentGear - 1`; `RPMS` = `Drivetrain::getEngineRPM()` 0x140266b80 =
`(float)drivetrain.engine.velocity * 0.15915507 * 60.0` (`GEAR` and `RPMS` only in the
ks_porsche_panamera steer-brake file); `CONST`. (The other inputs are listed in section 4.)
One file uses a name acs.exe does not know: vrc_formula_alpha_2026_csp `ctrl_ebb.ini` has
`INPUT=SCRIPT_11` (a mod car written for a patched game), which takes the unknown-name path
described in section 4. Followed through for that file (one stage, `ADD`, `LUT=(| 0=0 | 1=1 |)`,
limits 0..1, setup.ini `[FRONT_BIAS]` MIN=0 MAX=100): the stage value is `lut.getValue(0.0)` = 0,
so in the unpatched acs.exe this car's bias would be 0.0, all pedal torque on the rear wheels.
That is derived from the code, not run.

### 5.4 Cockpit adjustment - `BrakeSystem::setManualFrontBias(clicks)`

```
if hasCockpitBias:
    if biasOverride == -1.0: biasOverride = frontBias
    b = (float)clicks * biasStep + biasOverride
    biasOverride = b > limitUp ? limitUp : (b < limitDown ? limitDown : b)
```

Path to here (confirmed in the pseudo-C): the controls provider sets
`controls.brakeBalanceUp/Dn` -> `Car::getPhysicsState` copies them into bits 17 / 18 of
`physicsState.actionsState.state` -> `BrakeBiasNotifier::update` 0x1400db510 (graphics side) sees
a rising edge and calls `CarAvatar::setFrontBias(+1 or -1)` 0x1400da1f0 -> that queues a lambda
(0x1400d0890) with `PhysicsAvatar::executeOnPhysicsThread` 0x140121f20 -> the lambda calls
`setManualFrontBias(clicks)`. So the click count is always +1 or -1. `reset()` puts
`biasOverride` back to -1.
The queue is emptied by `PhysicsAvatar::stepCommandQueue` 0x1401223e0, which
`PhysicsAvatar::PhysicsAvatar` 0x140120930 registers as a handler of the physics engine's
`evOnPreStep` event, so a click (and a gentle-stop request, section 6) takes effect at the
start of the next physics step, before `Car::step`.

### 5.5 How the torque acts on the wheel (confirming `docs/map/tyre.md`)

In `Tyre::step` (members confirmed in the pseudo-C):

```
brake = max(inputs.brakeTorque * absOverride, inputs.handBrakeTorque)       # the larger one, not the sum
s     = sign(status.angularVelocity)                                        # -1, 0 or +1
modelData.version <  10: feedbackTorque = (status.loadedRadius * status.Fx - s * brake) + status.rollingResistence
modelData.version >= 10: feedbackTorque = status.rollingResistence - (s * brake + localMX)
feedbackTorque += inputs.electricTorque
status.feedbackTorque = feedbackTorque
```

- Not driven wheel: `Tyre::updateAngularSpeed`: `omega += feedbackTorque / data.angularInertia * dt`;
  if the sign of `omega` changed since the last step the wheel becomes locked
  (`status.isLocked = true`, `omega = 0`); `|omega| < 1` is multiplied by 0.9 every step.
  `Tyre::updateLockedState` releases a locked wheel unless
  `|loadedRadius * Fx| <= |brake|` and `|omega| < 1` and the wheel is not driven.
- Driven wheel: `Tyre::step` calls `updateLockedState` and does not integrate the wheel speed; it
  then sets `status.isLocked = true` when the sign of `angularVelocity` differs from the sign of
  `oldAngularVelocity` and `totalHubVelocity < 1`. The wheel speed comes from the
  drivetrain, which uses `status.feedbackTorque` of the driven tyres. `Drivetrain::step2WD` also
  reads the brake inputs directly: when both driven tyres are locked it compares
  `absOverride*brakeTorque + handBrakeTorque` summed over the two tyres (a sum here, not a max)
  with `|ratio| * engine out torque`, and unlocks both if the brakes are weaker or the car is
  faster than 1 m/s (details belong to the drivetrain map).
  `Drivetrain::step4WD` and `step4WD_new` have no such test: every step they write the shaft
  speeds into `status.angularVelocity` of all four tyres and set `status.isLocked = false`; no
  read of `inputs.*` or `absOverride` shows up by name in their pseudo-C (so, as far as I can see,
  the brake reaches a 4WD car only through `feedbackTorque`; not checked in the disassembly).
- Reaction on the suspension: at the top of `Tyre::step`, when the wheel is not locked and
  `car->torqueModeEx == reactionTorques`, the torque
  `(inputs.electricTorque + inputs.brakeTorque + inputs.handBrakeTorque)` about the wheel axis
  (`worldRotation.M11..M13`) is applied to the hub (`ISuspension` slot +0x20, `addTorque`). Here it
  is the plain sum, without `absOverride` and without `sign(angularVelocity)`.

### 5.6 Units and signs

- All brake torques are positive magnitudes in Nm. The direction is supplied by the tyre
  (`sign(angularVelocity)`), so a brake can never spin a wheel up; at exactly zero wheel speed the
  brake term is zero and the lock logic holds the wheel.
- `controls.brake`, `controls.handBrake`, `electronicOverride`: 0..1. Bias values: 0..1 fraction to
  the front. Temperatures: degC. `absOverride`: 0..1 (ABS and AI only ever write 0 or 1).
- `inputs.electricTorque` is signed: it is added to `feedbackTorque` as is, so a positive value
  raises `angularVelocity`.

---

## 6. Connections to other systems

Position in `Car::stepComponents`: `BrakeSystem::step` is the first call, `EDL::step` the second,
then suspensions and `Tyre::step`. Everything that runs after `Tyre::step` and writes a brake
input therefore takes effect one step later.

### Who writes the brake system's inputs

| Input | Writer | When |
|---|---|---|
| `Car::controls.brake`, `.handBrake` | the controls provider (`ICarControlsProvider`: wheel/keyboard/pad, `AIDriver::acquireControls` 0x14029baa0, drag AI) through `Car::pollControls` | start of `Car::step` |
| `Car::controls.brake = 1` (gas 0, steer 0, clutch 0) | `Car::step` when `isControlsLocked` or `physicsTime < lockControlsTime` (the test is made before `pollControls`, the write after it, and only if the car has a controls provider). Set through `Car::lockControls` 0x1402745f0 / `Car::lockControlsUntil` 0x140274600. Callers: `PenaltyManager::addJumpStartPenalty` 0x140265a90 (jump start in teleport-to-pit mode: `forcePosition` to the pits, then `lockControlsUntil(20000.0, session start time)`), and lambdas in `PitStop.obj`, `CarAvatar.obj` and `ACClient.obj` (not read) | after `pollControls` |
| `Car::controls.brake = 0.2` (gas 0) | `Car::step` when `isGentleStopping`. The flag is set from the game side by `CarAvatar::setGentleStop` 0x1400da280, which queues a lambda (0x1400d0960) on the physics thread; its callers are `DragMode`, `DriftMode`, `TimeAttack`, `TimeLimitedTest`, `ACClient` and `UDPCommandListener` functions (not read) | after `pollControls` |
| `Car::controls.brake = 0` | `Car::pollControls` when `isControlsLocked` or `blackFlagged` is set (then overwritten by the rule above if locked). It zeroes gas, brake, steer, clutch, gearUp, gearDn and kers but not `controls.handBrake`, which keeps the last value the provider wrote, so a handbrake that was on stays on | start of `Car::step` |
| `param->brake = clamp(brakeRequest, 0, 1)` | `AIDriver::acquireControls`, every call. `brakeRequest` itself is only recomputed on every 5th call (`freqCounter`; `AIDriver::stepGasBrake` is then called with `dt * 5`), so the AI's pedal changes at most every 5 physics steps | inside the AI controls provider |
| `param->brake = 1` (gas 0) | `AIDriver::stepSuperhuman` while `accidentStopCounter > 0` (by its name: the AI stopping after an accident). `stepSuperhuman` is called at the end of every `acquireControls` call, outside the 5-step skip | inside the AI controls provider |
| `BrakeSystem::electronicOverride` | `SpeedLimiter::step`: on a pit-lane surface above 81 km/h it writes `RaceEngineer::getOptimalBrake()` 0x14027c320 | after the tyres, used next step |
| `BrakeSystem::rearCorrectionTorque` | `ERS::step`: `(ERS::rearCorrectionTorque * ERS::kineticRecovery) * drivetrain.ratio` when the gear ratio is not 0 | after the tyres, used next step |
| `BrakeSystem::frontBias` | `SetupManager` setup item `FRONT_BIAS` (written as `multiplier * newValue` = `0.01 * newValue` by `SetupManager::step` when the item is attached and the product differs from the current value); `ACPlugin::setBrakeBias` 0x1400bfb80 writes it directly; the `AIDriver` constructor attaches the `FRONT_BIAS` item and sets its `newValue` when the car's ai.ini has `[BRAKE_BIAS] VALUE` (section 4) | setup changes |
| `BrakeSystem::brakePowerMultiplier` | `SetupManager` setup item `BRAKE_POWER_MULT`; `AIDriver::AIDriver` 0x140298c60 and `AIDriver::stepSetup` 0x1402a2020 force it to 1.0 for AI cars | setup changes / AI |
| `BrakeSystem::biasOverride` | `BrakeSystem::setManualFrontBias` (cockpit buttons, via `CarAvatar`), `BrakeSystem::reset` | on input |
| `Tyre::aiMult` (gate for brake temps) | `AIDriver::stepGasBrake` 0x1402a08e0 | AI |
| `tyres[i].status.load`, `.angularVelocity`, `.slipAngleRAD` | `Tyre::step` (previous step) | - |

### Every writer of `Tyre::inputs` and `Tyre::absOverride` (grep over all pseudo-C)

| Member | Writer | What |
|---|---|---|
| `inputs.brakeTorque` | `BrakeSystem::step` | sets all four every step |
| | `BrakeSystem::stepTemps` | multiplies by the fade curve |
| | `EDL::step` | adds `outLevel * brakeTorquePower` (or `...Coast` when `accG.z <= 0`) to the faster of its two wheels (`leftTyreIndex` / `rightTyreIndex`) |
| | `Tyre::Tyre` 0x14026dbd0 | 0 at construction |
| `inputs.handBrakeTorque` | `BrakeSystem::step` | rear wheels only |
| | `Tyre::Tyre` | 0 at construction |
| `inputs.electricTorque` | `ERS::step` | `tyres[0]` and `tyres[1]` only (front motors, when `frontTorqueLUT` has points) |
| | `Tyre::Tyre` | 0 at construction |
| `absOverride` | `ABS::step` | 0 or 1 per wheel, see below |
| | `AIDriver::stepSuperhuman` | AI's own ABS: 1, or 0 when `status.ndSlip > 1` and (wheel driven or it carries more than 5 % of the total load), if `useAbs` or no brake mistake is in progress |
| | `Tyre::init` 0x140280650, `Tyre::reset` 0x140283380 | 1.0 |

### ABS interface (the class itself is in the electronics map)

- `ABS::step` runs late in `stepComponents` (after `Drivetrain::step` and the anti-roll bars), so
  the tyre sees its result on the next step.
- It only acts every `ABS::frequency` seconds (`1 / RATE_HZ`, default 0.05); between updates
  `absOverride` keeps its last value.
- Active and present and speed above 20 km/h: a wheel whose `status.slipRatio` is below
  `-slipRatioLimit` gets `absOverride = 0`, otherwise 1. `channels` = 4: per wheel; 2: per axle
  (either wheel of the axle triggers both); 1: all four together.
- Otherwise all four are set to 1.0 (and `ABS::currentValue = 1`). A `channels` value other than
  1, 2 or 4 leaves `absOverride` untouched; the default is 4 (`Car::Car`).
- Each update also stores `isActive = isActive && isPresent` back into the `ABS` object.
- AI cars: `AIDriver::AIDriver` sets `car->abs.isActive = false`, so on an AI car `ABS::step` only
  ever writes 1.0. The values the tyre sees there come from `AIDriver::stepSuperhuman`, which runs
  inside `Car::pollControls` (before `Tyre::step`) and rewrites all four every time it runs.
  `pollControls` does not call the provider while `isControlsLocked` or `blackFlagged` is set;
  in those states the AI's last values only survive until the next `ABS::step` update (one
  `ABS::frequency` period, 0.05 s by default), which writes 1.0 to all four because
  `abs.isActive` is false.
- Nobody multiplies `inputs.brakeTorque` in place. The product `brakeTorque * absOverride` is
  formed by the readers: `Tyre::step`, `Tyre::updateLockedState`, `Drivetrain::step2WD`,
  `Kers::step`, `ERS::step`.
- `ABS::isInAction` 0x14028f5d0 (any `absOverride != 1`) feeds the pedal vibration in
  `Car::pollControls`. `CarAvatar::isAbsInAction` 0x1400d8ae0 makes the same test for the
  graphics side.

### Penalties

No penalty code shows up among the writers of the brake torques, `electronicOverride` or
`absOverride` (grep by member name over all pseudo-C). The penalty branch of `Car::step`
(`penaltyTime > 0`) writes no brake value: the only writes to `controls.brake` in `Car::step` are
the two rows of the table above. The one penalty path that reaches the brakes is the jump-start
penalty: it locks the controls, which forces `controls.brake = 1`.

### Who reads the brake system's outputs

| Output | Reader |
|---|---|
| `tyres[i].inputs.*`, `absOverride` | `Tyre::step`, `Tyre::updateLockedState` (wheel torque, lock state, hub reaction torque) |
| `absOverride * inputs.brakeTorque` (+ `handBrakeTorque`) | `Drivetrain::step2WD` (release of locked driven wheels) |
| sum over 4 wheels of `absOverride * brakeTorque` | `Kers::step` (charging: `/ brakeForMaxCharge`) |
| `|brakeTorque[2]| * absOverride[2] + |brakeTorque[1]| * absOverride[1]`, divided by the gear ratio | `ERS::step` (kinetic recovery). The indices are 2 and 1 (left rear and right front), confirmed in the disassembly (`Car`+0x1bd8 and `Car`+0x1380). |
| `brakePower`, `frontBias` | `RaceEngineer::getOptimalBrake` (callers: `AIDriver::stepGasBrake`, `SpeedLimiter::step`, `KeyboardCarControl::acquireControls` 0x14010f2e0), `RaceEngineer::getMaxBrakingForce` 0x14027c1f0, `Kers::init` |
| `getFrontBias()` | `SharedMemoryWriter::updatePhysics` 0x140186ef0 (`brakeBias`), `BrakeBiasNotifier::update` 0x1400db510. This is the static/cockpit bias, not the EBB result and not clamped. |
| `discs[i].t` | `Car::getPhysicsState` 0x140270d70 (copied out; the state struct has `discTemps[4]`), `SharedMemoryWriter::updatePhysics` (the shared-memory struct has `brakeTemp[4]`; exact field mapping is the telemetry map's job), the developer telemetry form (`FormTelemetry::initChannels`) |
| `ebbInstant` | `Telemetry::step` 0x1402bf5b0 (`* 100`; the `EBB_INSTANT` channel is only created, in `Telemetry::init` 0x1402bc010, when `isUsingEBB`). Because `ebbInstant` is only written in `Internal` mode, the channel reads a constant 50 (constructor value 0.5) on every `ctrl_ebb.ini` car: it is not the bias the controller chose. |
| `physicsState.brake` (copy of `controls.brake`) and `physicsState.wheelAngularSpeed` | `BrakeDiscGraphics::update` 0x14005d930: glow target = `maxGlow * brake * clamp((|wheelAngularSpeed| - 10) / 150, 0, 1)`. The glow is not driven by `discs[i].t`. |

---

## 7. Port notes

**Effort: S** for `BrakeSystem` itself (the two per-step functions are about 1.1 kB of code,
plain float maths, one ini file). **S/M** if `DynamicController` is counted here: its `eval` is small, but
`getInput` touches many car signals (suspension travel, engine rpm, gear) and the class is shared
with six other users, so it should be ported once as its own unit.

Port before it:
- Tyre (done): `Tyre::inputs`, `absOverride`, `status.load`, `status.angularVelocity`.
- `Curve` (`getValue`, the inline `(|x=y|)` parser of `INIReader::getCurve`) - already needed by
  the tyre.
- A `Car` shell with `controls`, the cached speed, `ksPhysics->ambientTemperature`.
- `DynamicController` only for the 10 EBB cars and the 3 steer-brake cars. The F2004 needs none
  of it and has no temperature data: for that car the whole system is five multiplications.

Port together with / after it: `ABS::step` (tiny, same file family), `EDL::step`,
`SpeedLimiter` (needs `RaceEngineer::getOptimalBrake`), `ERS` (`rearCorrectionTorque`).

Order of float operations that must be kept for bit-exact results (from the disassembly):
`(brakePower * brakePowerMultiplier) * pedal`; front `= torque * bias`; rear
`= (1 - bias) * torque - rearCorrectionTorque`; cooling
`((speed*3.6) * coolSpeedFactor + 1) * coolTransfer`, then `((ambient - t) * cool) * dt + t`;
heating `((|omega| * (T * torqueK)) * 0.001) * dt + t1`. The constants are the float literals
3.6 (0x40666666), 10.0, 0.001 (0x3a83126f), 0.003 (0x3b449ba6), 0.01 (0x3c23d70a), 1.1 (0x3f8ccccd).

Bit-exactness trap in the setup path (confirmed in `SetupItem::SetupItem` 0x1402cb170 and the
loop 0x140288b20): a setup item starts with `newValue = *target / multiplier`, and once it is
attached `SetupManager::step` writes `multiplier * newValue` back whenever that differs from the
target. In single precision this round trip is not always the identity: `FRONT_SHARE=0.72` is
0x3f3851ec, but `0.01f * (0.72f / 0.01f)` is 0x3f3851ed, and `0.01f * 72.0f` (a setup file value
of 72) is 0x3f3851eb. So on a car whose `FRONT_BIAS` item is attached, `frontBias` can sit one
unit in the last place away from the brakes.ini value (0.55, the F2004 value, survives both
ways). Which cars and sessions get the item attached was not traced (open question 8).

Bit-exact oracle (same in-process approach as the tyre oracle):
- Call `BrakeSystem::BrakeSystem` 0x14026be20 on the `brakeSystem` slot of a zeroed `Car`-sized
  buffer (0x3ea0 bytes). The constructor is needed because `step` calls
  `std::ios_base::operator bool` on the embedded `tempRunFile` stream (MSVCP120 import), which
  must be a real constructed object.
- Either call `BrakeSystem::init` 0x14028d690 with a fake `Car` whose `unixName` points at an
  extracted car folder under a `content/cars/<car>/data/` tree in the working directory (it uses
  `INIReader`, `Path::fileExists`, `Car::getConfigPath`; leave `configName` empty), or skip
  `init` and write the members directly from the ini values.
- Per case, fill in the fake `Car`: `controls.brake`, `controls.handBrake`,
  `tyres[i].status.load`, `tyres[i].status.angularVelocity`, `tyres[0].aiMult`,
  `valueCache.speed`, and a fake `PhysicsEngine` at `ksPhysics` with `ambientTemperature` at
  +0x100; plus the `BrakeSystem` members `biasOverride`, `electronicOverride`,
  `rearCorrectionTorque`, `brakePowerMultiplier`.
- Call `BrakeSystem::step(dt)` 0x14028e640 and record `tyres[i].inputs.brakeTorque`,
  `tyres[2..3].inputs.handBrakeTorque`, `discs[i].t`, `ebbInstant`. Run several steps in a row so
  the temperature integration and the one-step `electronicOverride` reset are covered.
- For EBB / steer-brake cars also fill the signals `DynamicController::getInput` reads
  (`tyres[i].status.slipAngleRAD`, `.slipRatio`, `controls.steer`, `steerLock`, `accG`,
  `drivetrain.currentGear`, and `drivetrain.engine.velocity` for `RPMS`, which the
  ks_porsche_panamera steer-brake file uses: `Drivetrain::getEngineRPM` is a plain member read,
  no vtable); the inputs that call into the suspension through its vtable (`AVG_TRAVEL_REAR`,
  `SUS_TRAVEL_*`, slot +0x48 `getStatus`) need a fake object, but no brake controller in
  `cardata/` uses them.
- The wheel side (`feedbackTorque`, lock state) is already covered by the tyre oracle: drive it
  with non-zero `inputs.brakeTorque`, `handBrakeTorque` and `absOverride` values.
- `ABS::step` 0x14028f610 can be tested the same way (needs `tyres[i].status.slipRatio`, the
  cached speed and the `ABS` members).

---

## 8. Open questions

1. **`tempRunFile` on the first step.** `BrakeSystem::step` calls `saveTempsRunFame` whenever the
   stream's `operator bool` is true. A default-constructed, never-opened `std::ofstream` reports
   good until the first failed write, so my reading is that the function runs once on the very
   first step, the write fails, and it never runs again unless the developer app opens the file.
   This is standard-library behaviour I did not execute. It has no effect on the physics either
   way (the function only reads).
2. **Initial disc temperature.** The constructor sets `t = 0`; `reset()` sets the ambient
   temperature and is called from `Car::forcePosition`. I did not confirm that `forcePosition`
   always runs before the first physics step of a session (its callers are a lambda in
   `CarAvatar.obj`, `Car::step` for black flags, `PenaltyManager::addJumpStartPenalty` and the AI).
3. **When the queued `setManualFrontBias` runs.** Resolved by the reviewer (section 5.4): the
   queue is emptied in an `evOnPreStep` handler, i.e. at the start of `PhysicsEngine::step`,
   before `Car::step`.
4. **`[EBB]` internal mode is unused** by all 113 cars, so its formula (front load share x
   multiplier, above 10 km/h) is confirmed from code only, with no data to sanity-check it.
5. **`ERS::step` uses `tyres[2]` and `tyres[1]`** (left rear + right front) for the braking part
   of kinetic recovery. Confirmed in the disassembly; whether it is intended (one wheel per axle)
   or a slip for `[2]` and `[3]` is unknown. Belongs to the ERS map, noted here because it reads
   brake outputs.
6. **Meaning of `MAX_TORQUE`.** The code gives `MAX_TORQUE * bias` to each front wheel and
   `MAX_TORQUE * (1 - bias)` to each rear wheel. Calling that "one front plus one rear wheel" is
   my wording; the SDK comment only says "Maximum Brake torque in Nm".
7. **Steer-brake side.** Positive controller output goes to `tyres[3]`, negative to `tyres[2]`.
   That `[3]` is the right rear and that positive `STEER_DEG` means steering right is taken from
   the wheel order, not checked against a running car.
8. **Setup path: who attaches the items.** The copy itself is now read (loop 0x140288b20, section
   7): `target = multiplier * newValue` for attached items. `SetupManager::init` 0x140289290 calls `initItems(false)`, so items start with `attached = false`, except that `SetupManager::initItems` 0x140289570 itself sets `attached = true` on every `INTERNAL_GEAR_n` item and on `FINAL_RATIO` right after creating them (multiplier 1.0, so the write-back is the identity). The only other write of `attached = true` visible by name is in `AIDriver::AIDriver` (ai.ini `[BRAKE_BIAS]`); the setup-screen gear lambdas and the pit-stop lambda only write `newValue`. For the player's car it must happen when a setup
   is applied (the other callers of `SetupManager::getSetupItem` are lambdas in `PitStop.obj` and
   the setup screen), which I did not read. Until that is known it is unverified whether a player
   car that never opens the setup screen runs with the raw brakes.ini `FRONT_SHARE` or with the
   round-tripped value. Left to the setup map.
9. **`AIDriver::stepSuperhuman` call rate.** Resolved by the reviewer: `acquireControls` has a
   5-call skip (`freqCounter`) around the planning functions (`stepGasBrake` and others), but
   `stepSuperhuman` is called after that block on every call, and the function has a single
   `return` at its end. So `absOverride` is rewritten on every physics step in which
   `Car::pollControls` calls the provider. Still open: whether an AI driver is ever attached to
   the player's car (pit lane, end of session). If it is, `aiMult` stays above 1 afterwards and
   the player's brake temperatures stop updating (section 5.1).
10. **`ABS::valueCurve` / `currentValue`** are not used by `ABS::step` to scale `absOverride`
    (only 0 and 1 are written). Their use in `ABS::cycleMode` is left to the electronics map.
11. **`electronicOverride` above 1.** `RaceEngineer::getOptimalBrake` is not clamped and
    `BrakeSystem::step` does not clamp the pedal, so the pit limiter could ask for more than
    `MAX_TORQUE`. Read from the formula only; no value was computed for a real car in the pit lane.
12. **Unknown controller input (`SCRIPT_11`).** Resolved by the reviewer: with
    `INIReader::crashAtError` off the constructor falls through to `COMBINATOR`, `LUT`, `FILTER`
    and the limits, pushes the stage and sets `ready` (sections 4 and 5.3). Not known: the value
    of `INIReader::crashAtError` in a normal game session. Only one mod car is affected.
13. **4WD and locked wheels.** `Drivetrain::step4WD` / `step4WD_new` clear `isLocked` on all four
    tyres every step (section 5.5). Whether a 4WD car can therefore never hold a locked wheel
    belongs to the drivetrain map; it was read from the pseudo-C only.
14. **Member-name grep.** The "every writer" table comes from a grep for member names in the
    pseudo-C. A write through a raw offset would not show up (that is how `isGentleStopping` is
    set, found only through the function name). `ERS::step` was the one place checked in the
    disassembly.
15. **Choices made here:** `DynamicController` is documented in this file as far as the brakes
    need it (eval, the ini format, the inputs brake controllers use) because no other map owned
    it at the time of writing; the full input list and the other users should live in one place
    later. Addresses of virtual `ISuspension` slots are quoted from `docs/map/tyre.md`, not
    re-derived.
