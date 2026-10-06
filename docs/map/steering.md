# Steering and force feedback

Source: `acs.exe` + `acs.pdb`, read through the local material only: pseudo-C in `re/decomp/`
(`SteeringSystem`, `FFPostProcessor`, `Car`, `PhysicsEngine`, `DynamicController`, `Suspension*`,
`DICarControl`, `InputDevice`, `SignalGenerator`), struct layouts in `re/types/`, `tools/re_query.py`
and `tools/disasm.py` for constants and operation order, `tools/ini_survey.py` over the 113 cars in
`cardata/`, the commented example car in the SDK and the game's `system/cfg` and `cfg` folders.
Copies of the pseudo-C and four disassemblies cited here: `re/car/steering/`.
Nothing in the game folder, the Ghidra project or git was changed. No Rust was written.

Words used below: "confirmed" = read in the pseudo-C or the disassembly; "interpretation" = my
reading of what the code is for.

---

## 1. Plain-English summary

The driver's steering input arrives as one number between -1 and +1 (one lock to the other).
Once per physics step the car multiplies it by the steering-wheel lock and divides by the steering
ratio, which gives the angle the front wheels are asked to take, in degrees.
The steering system turns that angle into a sideways distance in metres and slides the inner end of
both front steering rods by that distance; the rods have a fixed length, so the rigid-body solver
drags the wheels round during the same step.
There is no formula for the wheel angle and none for Ackermann: how much each wheel really turns
follows from where the rod ends sit in the suspension geometry.
A car with a `ctrl_4ws.ini` file also slides the rear rods, by an amount that a small programmable
controller works out from steering angle, throttle, rear slip and speed (2 of 113 cars).
In the other direction, every tyre force on a front wheel is also booked as a twist about that
wheel's steering axis. After the fourth tyre has been stepped the car adds the left and right
twists, optionally smooths them, adds an optional "spinning wheels resist being turned" term and a
flat-spot shake, scales the result with the car's `FFMULT`, bends it with the car's `STEER_ASSIST`
exponent and fades it out when the car is almost stopped.
That one number is the force feedback. It is handed, together with a damper strength that is highest
when standing still and falls to its minimum (zero as shipped) by 10 km/h, to whoever provides the
controls.
A real wheel then adds kerb, road, slip and ABS vibrations, a minimum force, the user's gains, an
optional gamma or lookup-table curve and a soft lock, and sends it to DirectInput.
AI and keyboard drivers ignore it, but the number is still computed for every car that has a driver
object attached.

---

## 2. Classes

| Class | Size | What it is for |
|---|---|---|
| `SteeringSystem` | 0x40 | Per car. `linearRatio` +0x00 (m of rod travel per degree), `car` +0x08, `has4ws` +0x10, `ctrl4ws` +0x18 (`DynamicController`). One method per step. Physics. |
| `Car` (steering members) | 0x3ea0 | `finalSteerAngleSignal` +0x08, `userFFGain` +0xec, `ksPhysics` +0x138, `controls` +0x140 (`steer` at +0x16c), `steerLock` +0x174, `steerRatio` +0x178, `steerLinearRatio` +0x17c, `ffMult` +0x1e4, `suspensions` +0x2c88, `steeringSystem` +0x3bf0, `lastGyroFF` +0x3c44, `lastFF` +0x3c48, `lastPureMZFF` +0x3c4c, `steerAssist` +0x3c70, `isControlsLocked` +0x3d14, `blackFlagged` +0x3d20, `controlsProvider` +0x3d80, `mzCurrent` +0x3d98, `vibrationPhase` +0x3e18, `slipVibrationPhase` +0x3e1c, `flatSpotPhase` +0x3e20, `lastSteerPosition` +0x3e34, `valueCache.speed` +0x3e90. |
| `CarControls` | 0x34 | Driver inputs of this step. `steer` +0x2c is -1..1. Filled by the controls provider. |
| `CarControlsInput` | 0x8 | What the provider is told when asked for controls: `steerLock` +0 (the car's), `speed` +4. |
| `ICarControlsProvider` | 0x18 | Interface to "who drives": `ffEnabled` +0x08, `ffFilter` +0x0c, `suppressPenalties` +0x10, `isAutoclutchNeeded` +0x11, `useFakeUndersteerFF` +0x12, `keyboardEnabled` +0x13. Vtable 0x1404b2030 (slots in section 6). Implemented by `DICarControl` (wheel), `JoypadCarControl`, `KeyboardCarControl`, `AIDriver`, `AIDriverDrag`. |
| `VibrationDef` | 0x14 | Five vibration levels built in `Car::pollControls`: `curbs`, `gforce`, `slips`, `engine`, `abs`. |
| `SteerMzLowSpeedReduction` | 0x8 | `speedKMH`, `minValue`: fade of the force feedback near standstill. Lives in `PhysicsEngine` +0x13c. |
| `PhysicsEngine` (FF members) | 0x278 | `flatSpotFFGain` +0x138, `mzLowSpeedReduction` +0x13c, `gyroWheelGain` +0x148, `spinTorqueGain` +0x14c (no reader found), `damperMinValue` +0x150, `damperGain` +0x154. Global, not per car. |
| `DynamicController` | 0x28 | Shared programmable formula (`ctrl_*.ini`). Here: the rear-steer controller. Described in `docs/map/brakes.md` 5.3 and `docs/map/drivetrain.md`. |
| `DynamicControllerStage` | 0xa0 | One stage of it: `inputVar`, `combinatorMode`, `lut`, `filter`, `upLimit`, `downLimit`, `currentValue`, `constValue`. |
| `FFPostProcessor` | 0x90 | Optional last reshaping of the wheel force: `enabled` +0, `gamma` +4, `type` +8 (`Gamma` = 0, `Lut` = 1), `lut` +0x10 (`Curve`). Compiled into the physics library but owned and used only by `DICarControl`. Not physics. |
| `DICarControl` | 0xb50 | The DirectInput wheel provider. FF members: `ffGain` +0xa50, `steerLock` +0xa58, `ffUpgrades` +0xa68 (curbs, gforce, slips, abs gains), `minFF` +0xa78, `centerBoostGain` +0xa7c, `centerBoostRange` +0xa80, `useSoftLock` +0xa84, `ffPostProcessor` +0xa88, `ffCounter` +0xb20, `ffInterval` +0xb24, `currentVibration` +0xb28, `currentLock` +0xb2c, `lastSpeed` +0xb48. Hardware side: belongs to the input atlas, listed here only as the receiver. |
| `ISuspension` and its 4 implementations | - | Only three slots matter here: +0x28 `setSteerLengthOffset`, +0x30 `getSteerTorque`, +0xa8 `getSteerBasis`. Everything else: `docs/map/suspension.md`. |

---

## 3. Key functions

### Loading / init

```
PhysicsEngine::PhysicsEngine                    0x140262430
├─ defaults: flatSpotFFGain 0.05, mzLowSpeedReduction {3.0, 0.5}, damperMinValue 0, damperGain 1
├─ gyroWheelGain = 0, spinTorqueGain = 0   (the 0.05 / 0.01 set a few lines earlier do not survive)
├─ system/cfg/assetto_corsa.ini [FF_EXPERIMENTAL] ENABLE_GYRO, DAMPER_MIN_LEVEL, DAMPER_GAIN
└─ PhysicsEngine::initLowSpeedFF                0x140263c90   [LOW_SPEED_FF] SPEED_KMH, MIN_VALUE

Car::Car                                        0x14026bf00
├─ defaults: steerLock 200, steerRatio 12, ffMult 0.003, steerAssist 1, userFFGain 1, mzCurrent 0, lastFF 0
├─ Car::initCarData                             0x140272b30   car.ini [CONTROLS] (section 4)
├─ ... suspensions built; each one calls its own setSteerLengthOffset(0) (see suspension map)
├─ tyres[3].onStepCompleted = lambda            0x14026eee0   → Car::onTyresStepCompleted
├─ SteeringSystem::init(car)                    0x1402b80d0
│   ├─ linearRatio = 0.003, car = car
│   └─ if <car data>/ctrl_4ws.ini exists:
│        DynamicController::DynamicController(car, path)  0x1402af330 → ctrl4ws; has4ws = true
└─ steeringSystem.linearRatio = car.steerLinearRatio          (right after init, in Car::Car)

Car::setControllerProvider(provider)            0x140275a70   deletes the old provider, stores Car::controlsProvider,
                                                              fires evOnControlsProviderChanged
   called from CarAvatar::initControls          0x1400d6f30
   and, queued onto the physics thread, from CarAvatar::setControlsProvider 0x1400d9f90 (swap at run time)

DICarControl::DICarControl                      0x14007ddd0   (wheel only; input atlas)
├─ FFPostProcessor::FFPostProcessor             0x140279560   Documents cfg/ff_post_process.ini
├─ system/cfg/assetto_corsa.ini [FORCE_FEEDBACK] FF_SKIP_STEPS, [SOFT_LOCK] ENABLED
└─ Documents cfg/controls.ini [STEER] FILTER_FF → ffFilter, FF_GAIN, LOCK, ...;
     [FF_ENHANCEMENT], [FF_ENHANCEMENT_2] UNDERSTEER → useFakeUndersteerFF, [FF_TWEAKS], [FF_SKIP_STEPS]
```

### Every physics step (dt = 0.003 s)

```
PhysicsEngine::step                             0x140264760
├─ Car::stepPreCacheValues (each car)           0x1402768c0   valueCache.speed = |body velocity|
├─ Car::step (each car)                         0x140275da0
│  ├─ Car::pollControls(dt)                     0x140274e70
│  │   ├─ provider->acquireControls(&controls, dt, &{steerLock, speed})   slot +0x08
│  │   │     writes controls.steer (-1..1)      DICarControl 0x14007fe70, AIDriver 0x14029baa0, ...
│  │   ├─ provider->setEngineRPM(rpm, limiter·0.75)                       slot +0x50
│  │   └─ builds VibrationDef, provider->setVibrations(&v)                slot +0x48
│  ├─ controls locked → controls.steer = 0
│  ├─ finalSteerAngleSignal = steerLock · controls.steer / steerRatio   (0 if not finite)
│  └─ Car::stepComponents(dt)                   0x1402764d0
│     ├─ BrakeSystem::step, EDL::step
│     ├─ each suspension->step                                 steerTorque = 0 at the top
│     ├─ Tyre::step × 4                         0x140283800    grip force and Mz add to steerTorque
│     │   └─ after tyre 3: Car::onTyresStepCompleted           0x140274cd0
│     │        ├─ Car::getSteerFF               0x140272180
│     │        │    ├─ suspensions[1]->getSteerTorque(), suspensions[0]->getSteerTorque()   slot +0x30
│     │        │    ├─ ksSawToothWave           0x14022d2a0   (flat-spot shake)
│     │        │    └─ powf                                    (steer assist curve)
│     │        ├─ low-speed reduction (Car::getSpeed 0x140272160)
│     │        └─ provider->sendFF(lastFF, damper, userFFGain)             slot +0x18
│     │             └─ DICarControl::sendFF     0x140080e40   → FFPostProcessor::getProcessedFF 0x140279a80
│     │                                                       → InputDevice::sendFF 0x14023f9b0
│     ├─ HeaveSpring::step × 2, DRS::step, AeroMap::step, Kers::step, ERS::step
│     ├─ SteeringSystem::step(dt)               0x1402b81b0
│     │    ├─ suspensions[0], [1]->setSteerLengthOffset(−signal·linearRatio)   slot +0x28
│     │    └─ if has4ws: r = DynamicController::eval 0x1402b0c00
│     │                  suspensions[2], [3]->setSteerLengthOffset(r)
│     └─ AutoBlip ... Drivetrain::step ... (rest of the list)
└─ core->step  (ODE): the solver moves the hubs to satisfy the reseated steering rods
```

Readers outside the step: `Car::getFinalFF` 0x140270960 (shared memory), `Car::getPhysicsState`
0x140270d70 (`lastFF_Pure`, `lastFF_Final`, `steer`), `Telemetry::step` 0x1402bf5b0.

---

## 4. Data it reads

### car.ini `[CONTROLS]` (`Car::initCarData` 0x140272b30; all 113 cars have all five keys)

| Key | Member | Conversion | When missing | Units / meaning | Values in `cardata/` |
|---|---|---|---|---|---|
| `STEER_LOCK` | `Car::steerLock` | none | 0 (the constructor's 200 is always overwritten) | degrees of steering wheel from centre to one lock | 180 .. 585, most common 450 |
| `STEER_RATIO` | `Car::steerRatio` | none | 0 → the division gives inf/NaN → signal forced to 0 | steering-wheel degrees per road-wheel degree. **Sign matters**: 19 of 113 cars use a negative ratio | -18.5 .. 26; size 10 .. 26 |
| `LINEAR_STEER_ROD_RATIO` | `Car::steerLinearRatio`, copied to `SteeringSystem::linearRatio` | none | 0 → 0.003 | metres of rod-end travel per degree of `finalSteerAngleSignal` | 0.0011 .. 0.0037 |
| `FFMULT` | `Car::ffMult` | × 0.001 | 0 → no force feedback | force-feedback gain per unit of steering torque | 0.81 .. 9.85 |
| `STEER_ASSIST` | `Car::steerAssist` | none | 0 → 1.0 | exponent of the force-feedback curve; 1.0 = linear | 1.0 in 106 cars; 0.76 .. 0.90 in 7 (Formula 1 cars, e.g. ks_ferrari_f2004 0.86) |

No version switch touches these keys. There is no power-steering key and no Ackermann key; the
strings `POWER_STEER`, `ACKERMAN`, `REAR_STEER` do not exist in the executable. `RACK_TRAVEL_LUT`
(1 car, a mod) is not a string in the executable either, so the game ignores it.

The SDK comments on `LINEAR_STEER_ROD_RATIO`: tune it by hand until the steer ratio shown by the
in-game suspension dev app matches `STEER_RATIO`. So the three numbers are meant to be consistent,
but the code does not enforce it.

### ctrl_4ws.ini (optional; 2 of 113 cars: ks_ferrari_812_superfast, ks_porsche_panamera)

A `DynamicController` file: sections `[CONTROLLER_0]`, `[CONTROLLER_1]`, ... until the first
missing index. Keys per section: `INPUT`, `COMBINATOR` (`ADD` / `MULT`), `LUT` (inline
`(|x=y|...)` or a file name), `FILTER`, `UP_LIMIT`, `DOWN_LIMIT`, and `CONST_VALUE` when
`INPUT=CONST`. Stored filter = `lagToLerpDeltaK(FILTER, 0.004, 0.003)` 0x14005d7c0
= `(1/0.003)·0.004·(1 − FILTER)·(1/0.003)`.
Both shipped files use four stages: `STEER_DEG` (ADD), then `GAS`, `SLIPANGLE_REAR_MAX`,
`SPEED_KMH` (all MULT). The result is the rear rod offset in metres (last-stage limits ±0.04 m for the Ferrari, ±0.011 m
for the Porsche).
The file's presence is the only switch (`Path::fileExists`), there is no key in car.ini.

### system/cfg/assetto_corsa.ini (global)

| [SECTION] KEY | Member | Rule | Shipped value |
|---|---|---|---|
| `[FF_EXPERIMENTAL] ENABLE_GYRO` | `PhysicsEngine::gyroWheelGain` | non-zero → 0.004, else 0.0. Read only if the section exists | 0 |
| `[FF_EXPERIMENTAL] DAMPER_MIN_LEVEL` | `damperMinValue` | read whenever the file opens, even without the section (then 0) | 0.0 |
| `[FF_EXPERIMENTAL] DAMPER_GAIN` | `damperGain` | same; without the key the gain becomes 0, not the constructor's 1.0 | 1.0 |
| `[LOW_SPEED_FF] SPEED_KMH` | `mzLowSpeedReduction.speedKMH` | only if the section exists; else 3.0 | 3 |
| `[LOW_SPEED_FF] MIN_VALUE` | `mzLowSpeedReduction.minValue` | only if the section exists; else 0.5 | 0.01 |
| `[FORCE_FEEDBACK] FF_SKIP_STEPS` | `DICarControl::ffInterval` | wheel only; overridden by controls.ini `[FF_SKIP_STEPS] VALUE` | 0 |
| `[SOFT_LOCK] ENABLED` | `DICarControl::useSoftLock` | wheel only | 0 |

### system/cfg/physics.ini: not read

The shipped file has `[TYRES] FLAT_SPOT_FF=0.05` (and `FLAT_SPOT_K`, `IS_TYRE_TEMP_ENABLED`), but
none of these key names, nor the file name `physics.ini`, exists as a string in `acs.exe` (checked
in both UTF-16 and ASCII). `PhysicsEngine::flatSpotFFGain` is written only by the constructor
(0.05). So the flat-spot force-feedback gain is a fixed 0.05.

### Documents `cfg/ff_post_process.ini` (wheel only, `FFPostProcessor::FFPostProcessor` 0x140279560)

Opened with `INIReaderDocuments`, so it lives in the user's Documents folder (`Assetto Corsa/cfg/`),
not in `system/cfg/`. A template is in the install's `cfg/` folder.

| [SECTION] KEY | Member | Default | Notes |
|---|---|---|---|
| `[HEADER] ENABLED` | `enabled` | false | template: 0 |
| `[HEADER] TYPE` | `type` | not initialised | `GAMMA` → `Gamma` (0), `LUT` → `Lut` (1) |
| `[GAMMA] VALUE` | `gamma` | 1.0 | only when TYPE=GAMMA; template 0.5 |
| `[LUT] CURVE` | `lut` | empty | only when TYPE=LUT; loaded with `INIReader::getCurve` 0x140235040 (inline `(|x=y|...)` or a `.lut` file of `input|output` lines; the template's file runs 0..1 on both sides, the code does not enforce a range) |

### Documents `cfg/controls.ini` (wheel only; only the two keys that reach physics code)

| [SECTION] KEY | Member | Rule | Template value |
|---|---|---|---|
| `[STEER] FILTER_FF` | `ICarControlsProvider::ffFilter` | clamped to 0..1 (`saturate`) | 0 |
| `[FF_ENHANCEMENT_2] UNDERSTEER` | `ICarControlsProvider::useFakeUndersteerFF` | non-zero → true | 0 |

The rest (`[STEER] FF_GAIN`, `LOCK`, `STEER_GAMMA`, `STEER_FILTER`, `SPEED_SENSITIVITY`,
`[FF_ENHANCEMENT] CURBS/ROAD/SLIPS/ABS`, `[FF_TWEAKS] MIN_FF/CENTER_BOOST_GAIN/CENTER_BOOST_RANGE`)
stays inside `DICarControl`.

### Other inputs

- Documents `cfg/user_ff.ini` `[<car folder name>] VALUE` → `CarAvatar::setUserFFGain` 0x1400dad50 →
  `Car::userFFGain` (player car only, set on the physics thread). 1.0 = 100 %. Two keys change it
  by ±0.01 (`Sim::onKeyDown` 0x14019a940 → `CarAvatar::modifyUserFFGain` 0x1400d90f0, clamp 0..200).
  The same function is reached from the Python call `ac_setCarFFB` 0x14012d890 (argument × 0.01,
  car 0) and from the buttons of the FFB app (`FFBForm`).
- `Car::ffMult` can be overwritten after loading (added by the reviewer, confirmed):
  `CarAvatar::setFFMult` 0x1400da160 queues `car.ffMult = value` onto the physics thread. Callers:
  the console variable `ffMult` (`Console::initCommands` 0x14018aa80; raw value, i.e. `FFMULT`/1000)
  and the FFB app's auto-adjust, which at each completed lap of car 0 sets
  `suggestedFFMult · 0.001`. `FFBForm::saveFFB` 0x140317750 writes `[CONTROLS] FFMULT` and
  `STEER_ASSIST` back into the car's ini with `WritePrivateProfileStringW` and resets the user gain
  to 1.0. All of this is user-interface driven; nothing changes `ffMult` during a normal session.
- Setup items `FF_GAIN` → `Car::userFFGain` (multiplier 1.0) and `STEER_ASSIST` → `Car::steerAssist`
  (multiplier 0.01) are created in `SetupManager::initItems` 0x140289570, but no `setup.ini` in
  `cardata/` has such a section (0 of 113).

---

## 5. Each step: inputs, maths, outputs

All arithmetic is 32-bit float unless marked. Constants are the literals in the code.

### 5.1 Steering input → requested wheel angle (`Car::step` 0x140275da0, inline)

```
controls.steer            written by the provider in pollControls, -1 .. +1
if controls are locked (isControlsLocked, or physicsTime < lockControlsTime):  controls.steer = 0
a = steerLock · controls.steer / steerRatio            degrees
if _fdtest(a) > 0  (a is inf or NaN):  a = 0
finalSteerAngleSignal = a
```

This is a nominal road-wheel angle. Nothing checks that the wheels really reach it.

Two details of the lock (confirmed): the zeroing in `Car::step` only runs when a provider is
attached (it also sets gas 0, brake 1, clutch 0), and when `isControlsLocked` or `blackFlagged` is
set `Car::pollControls` does not call `acquireControls` at all: it writes gas, brake, steer and
clutch = 0 itself and sends zero vibrations and `sendFF(0, 0)`. So a black-flagged car also steers
straight, although `Car::step` does not test `blackFlagged` for the zeroing.

### 5.2 Requested angle → rod offset (`SteeringSystem::step` 0x1402b81b0)

```
off = −(car.finalSteerAngleSignal · linearRatio)        metres; computed twice, same value
suspensions[0]->setSteerLengthOffset(off)                left front
suspensions[1]->setSteerLengthOffset(off)                right front
if has4ws:
    r = ctrl4ws.eval()                                   metres, not negated
    suspensions[2]->setSteerLengthOffset(r)
    suspensions[3]->setSteerLengthOffset(r)
```

The `dt` argument is not used. Example (ks_ferrari_f2004): 180 / 15 = 12 degrees at full lock,
× 0.00205 = 24.6 mm of rod travel each way.

What the suspension does with it (`Suspension::setSteerLengthOffset` 0x1402c3290, strut
0x1402c6540, multilink 0x1402caa40; the axle version is the shared empty function), confirmed and
described in `docs/map/suspension.md` 5.8:

```
carSteer.x = damageDirection·damageAmount + off + sign(refPoint.x)·toeOUT_Linear + baseCarSteerPosition.x
carSteer.y, .z = baseCarSteerPosition.y, .z
core->reseatDistanceJointLocal(steering joint, carSteer, tyreSteer)      0x1402cd480
```

So steering moves the body-side end of a fixed-length rod along the car's x axis (left/right).
Left and right get the same `off`; toe adds an opposite-signed constant per side; damage adds a
per-wheel constant. **Ackermann is only geometry**: it comes from where `WBCAR_STEER` and
`WBTYRE_STEER` sit relative to the steering axis in `suspensions.ini`.

Sign (interpretation): for a given `off` the wheel turns one way if the steering arm points
forward of the steering axis and the other way if it points back. The code has no flag for that;
the negative `STEER_RATIO` of 19 cars is how the data flips it.

### 5.3 Rear steer (`DynamicController::eval` 0x1402b0c00)

Same routine as for brakes and differentials (`docs/map/brakes.md` 5.3):

```
out = 0
for each stage:
    v = (inputVar == CONST(17)) ? constValue : lut.getValue(getInput(inputVar))
    if |v − currentValue| >= 0.001:
        k = clamp(filter · 0.003, 0, 1)                  = clamp(1.3333·(1 − FILTER), 0, 1)
        v = (v − currentValue)·k + currentValue
    currentValue = v
    combinator 0: out = 0;   ADD(1): out = out + v;   MULT(2): out = out · v
    if not (downLimit == 0 and upLimit == 0):  out = clamp(out, downLimit, upLimit)
```

Inputs the two shipped files use (`DynamicController::getInput` 0x1402b0d70): `STEER_DEG` =
`steerLock · controls.steer`; `GAS` = `controls.gas`; `SLIPANGLE_REAR_MAX` =
`max(|slipAngleRAD[2]|, |slipAngleRAD[3]|) · 57.29578`; `SPEED_KMH` = `getSpeed() · 3.6`.
Also available: `WHEEL_STEER_DEG` = `finalSteerAngleSignal`, `STEER` = `controls.steer`.
Each stage keeps one float of state (`currentValue`), so rear steer has memory between steps.

### 5.4 Tyre forces → steer torque (suspension side, interface only)

Per front wheel and step, `steerTorque` is zeroed at the top of the suspension's `step` and then
collects (confirmed at the call sites, 4th argument = `addToSteerTorque`):

| Source | Call | Adds to `steerTorque` |
|---|---|---|
| Tyre vertical load (`Tyre::addGroundContact`, call at 0x14027db8b) | `addForceAtPos(F, P, driven, 0)` | nothing |
| Forces the suspension puts on its own hub inside `step` (spring, damper, bump stop; see suspension map) | `addForceAtPos(.., 0, 0)` | nothing |
| Tyre grip force, V10 (`Tyre::addTyreForcesV10`, call at 0x14027f4d4) | `addForceAtPos(Fx·heading + Fy·right, contactPoint, driven, 1)` | `((P − centre) × F) · axis` |
| Tyre grip force, old model (`Tyre::addTyreForces`, call at 0x14027eb58) | same, at `contactPoint + trail·heading`, flag 1 | same; the trail is in the arm |
| Tyre aligning torque, V10 (call at 0x14027f52d) | `addTorque(Mz·normal)` | `axis · T` |
| V10 when `car.torqueModeEx != original` (`Tyre::addTyreForceToHub` 0x14027dc00) | `addLocalForceAndTorque` (slot +0xc0) instead of the grip-force call; `Mz` still goes through `addTorque` | double-wishbone and strut only (see suspension map). **Dead path**, see below |
| Brake + handbrake + electric torque when `car.torqueModeEx == reactionTorques` and the wheel is not locked (`Tyre::step`, call at 0x1402839cb) | `addTorque((brake + handBrake + electric) · wheel axle direction)` | `axis · T`. **Dead path**, see below |

`centre` and `axis` come from `getSteerBasis` (slot +0xa8): the line through the hub's upper and
lower ball joints (strut: foot to top mount; details in the suspension map 5.10). Units: N·m about
the steering axis.
`getSteerTorque` (slot +0x30) returns the sum: 0x1402c1d60 double wishbone, 0x1402c4ed0 strut,
0x1402c9940 multilink; the axle returns 0 (shared 4-byte function 0x14044f230).
Anything applied to the hubs after the fourth tyre in the same step (heave springs, anti-roll
bars) is too late: the force feedback has already been read, and the sum is zeroed at the next
step.

`Car::torqueModeEx` (+0x3d10) is written only by the `Car` constructor, twice, both times
`original` (0); no other function writes it and no ini key for it exists in the executable (the
`[_EXTENSION] TORQUE_MODE_EX` key found in one mod car's suspensions.ini is not a string in
`acs.exe`). So in this executable the two rows marked dead never run: the V10 grip force always
enters at the contact point with the steer-torque flag set, and brake and drive torques act only
on the wheel's spin, never on this sum.

### 5.5 Steer torque → force feedback (`Car::getSteerFF` 0x140272180)

Order of operations as in the disassembly (`re/car/steering/Car__getSteerFF__140272180.asm`):

```
st  = suspensions[1].getSteerTorque() + suspensions[0].getSteerTorque()
mzCurrent = (mzCurrent − st) · provider.ffFilter + st            # one-pole low-pass; filter 0 = none

pos  = steerLock · controls.steer                                 # steering wheel, degrees
gyro = ((pos − lastSteerPosition) · 333.33334 / |steerRatio|)
       · ((|ω1| + |ω0|) · tyres[0].data.angularInertia)
       · ksPhysics.gyroWheelGain                                  # ω = tyres[i].status.angularVelocity
lastSteerPosition = pos
lastPureMZFF = mzCurrent · 1.4
ff = −(mzCurrent + gyro) · 1.4

# flat-spot shake (flatSpot and blister are doubles, compared as doubles)
fs = max(flatSpot[0], flatSpot[1],  0.003 · max(blister[0], blister[1], blister[2], blister[3]))
w  = |ω0| + |ω1|
flatSpotPhase += w · 0.003                                        # literal, not the dt argument
if w > 7.0:
    ff += ksSawToothWave(flatSpotPhase, 12.56636) · (float)fs · ksPhysics.flatSpotFFGain
          · (load[1] + load[0]) · 0.5  + 1.0                      # the + 1.0 is in the code

# car gain and steer-assist curve
if steerAssist == 1.0:  ff = ff · ffMult
else:                   ff = sign(ff) · powf(|ff · ffMult|, steerAssist)      # sign(0) = 0
ff = −ff
lastGyroFF = gyro · 1.4

# optional "fake understeer" lightening
if provider != null and provider.useFakeUndersteerFF:
    u  = (ndSlip[1] + ndSlip[0]) · 0.5 − 1.0
    ff = ff / min(clamp(u, 0, 1) · 5.5 + 1.0, 2.5)
return ff
```

`ksSawToothWave(x, p) = ((x − (float)(int)(x/p)·p) − p·0.5) / (p·0.5)` 0x14022d2a0, a -1..1 ramp.
The period 12.56636 is 4π to six digits: the phase adds both front wheels' rotation, so one ramp per wheel turn.

Reading it plainly: with the default settings (filter 0, gyro off, no flat spot, assist 1)
`ff = (st0 + st1) · 1.4 · FFMULT / 1000` while the front wheels turn slower than 7 rad/s combined,
and `ff = ((st0 + st1) · 1.4 − 1.0) · FFMULT / 1000` above that (the `+ 1.0` of the flat-spot
line, negated by the final sign flip; corrected by the reviewer, the first draft left the offset
out here). For ks_ferrari_f2004 (`FFMULT` 3.752) the value reaches
1.0 at about 190 N·m of combined steering torque, before the 0.86 exponent lifts the small forces.

Notes:
- `333.33334` is the hard-coded step rate; the function has no `dt`.
- With `ENABLE_GYRO=0` (shipped) `gyroWheelGain` is 0, so `gyro` is 0 but is still computed.
- `flatSpotPhase` and `lastSteerPosition` are state. `flatSpotPhase` only ever grows.
- Rear tyres enter only through the blister maximum.
- The constant `+ 1.0` is added to the pre-gain value whenever the front wheels turn faster than
  7 rad/s combined, flat spot or not. After the gain and the final sign flip it is worth
  `−FFMULT/1000` in the returned value (assist 1). It reads
  like a slip of the pen in the original, but it is what the instructions do.
- `ffFilter` is read through `controlsProvider` without a null test at the top of the function; the
  only caller (`Car::onTyresStepCompleted`) has already tested the pointer.

### 5.6 Low-speed fade, damper, hand-over (`Car::onTyresStepCompleted` 0x140274cd0)

```
if controlsProvider == null:  mzCurrent = 0;  return
lastFF = getSteerFF()
if mzLowSpeedReduction.speedKMH != 0:
    t = clamp(speed · 3.6 / speedKMH, 0, 1)                       # speed = valueCache.speed, m/s
    lastFF = ((1 − minValue) · t + minValue) · lastFF
if not isControlsLocked and not blackFlagged:
    s = clamp(1 − speed · 3.6 · 0.1, 0, 1)                        # 1 at rest, 0 from 10 km/h up
    damper = ((1 − damperMinValue) · s + damperMinValue) · damperGain
    controlsProvider->sendFF(lastFF, damper, userFFGain)          # slot +0x18
```

`lastFF` is not clamped anywhere in physics. With the shipped ini the fade runs from 1 % at 0 km/h
to 100 % at 3 km/h.

### 5.7 Vibrations (`Car::pollControls` 0x140274e70; details in `docs/map/car_step.md` 5.4)

Built at the start of the step from the previous step's tyre state and handed to the provider
through slot +0x48. Short form:

```
vibrationPhase += speed · dt;  slipVibrationPhase += dt
curbs  = ksSawToothWave(vibrationPhase, mean vibrationLength over wheels whose surface has gain and length)
         · clamp(load[0],0,1) · clamp(load[1],0,1) · max vibrationGain · clamp(speed,0,1)²
gforce = sinf(vibrationPhase · 30) · clamp(|accG.y|, 0, 1)
slips  = sinf(slipVibrationPhase · 120) · clamp(0.4·m, 0, 1) · clamp(speed,0,1)
         with m = max(0, 0.75·ndSlip[i]) over the four tyres, squared when m <= 1
engine = clamp(engine.velocity · 0.15915507 · 60 / limiterRPM, 0, 1)
abs    = ksSquareWave(physicsTime, 100) · clamp(speed,0,1)   if ABS is present and acting, else 0
```

The wheel mixes them (`DICarControl::setVibrations` 0x140081010) into one `currentVibration`,
clamped to ±1, that is added to the force later. These never enter `lastFF`.

### 5.8 What the wheel does with it (`DICarControl::sendFF` 0x140080e40; input atlas, summary only)

```
if not validated:  return
ff = lastFF
if centerBoostGain != 0 and |axis| < centerBoostRange:  ff = ff · ((1 − |axis|/centerBoostRange) · centerBoostGain)
m  = clamp((lastSpeed·3.6 − 1) · 0.2, 0, 1) · minFF
if |ff| < m:  ff = sign(ff) · m                                   # minimum force
every (ffInterval + 1)-th call only; otherwise count and return
if not ffEnabled:  ff = 0
if soft lock is on and |currentLock| > 1:   InputDevice::sendFF(sign(currentLock), 1.0)
else:
    out = (ff + currentVibration) · ffGain · userFFGain
    if ffPostProcessor.enabled:  out = FFPostProcessor::getProcessedFF(out)
    InputDevice::sendFF(out, damper)             0x14023f9b0: clamp(out·10000, ±10000) → DirectInput
```

`FFPostProcessor::getProcessedFF(x)` 0x140279a80: not enabled → `x`; `Gamma` →
`sign(x) · powf(|x|, gamma)`; `Lut` → `sign(x) · lut.getValue(|x|)`; any other `type` → `x`.

`JoypadCarControl::sendFF` 0x1401dc370 ignores the force and only refreshes the pad rumble every
11th call. Keyboard and AI providers use the shared empty function.

### 5.9 The number called "final FF" (`Car::getFinalFF` 0x140270960)

```
finalFF = controlsProvider->getFFGlobalGain() · (lastFF · userFFGain)      # slot +0x28
```

The grouping is from the disassembly (`lastFF · userFFGain` first, then times the gain; the
pseudo-C prints it the other way round). `Car::getPhysicsState` computes
`gain · (userFFGain · lastFF)` at 0x14027108d, which gives the same bits. Neither tests
`controlsProvider` for null.

`getFFGlobalGain` is `DICarControl::ffGain` for a wheel (0x140080790) and the constant 1.0 for
keyboard, pad and AI (shared function 0x1402ac330). So `finalFF` contains: steer torque, filter,
gyro, flat spot, `FFMULT`, steer assist, fake understeer, low-speed fade, the wheel gain and the
per-car user gain. It does **not** contain: vibrations, minimum force, centre boost, the
gamma/LUT post-process, the soft lock, the ±1 clamp or the skipped steps.

---

## 6. Connections to other systems

### Inputs (who writes what this system reads)

| What | Written by | When |
|---|---|---|
| `Car::controls.steer` | `controlsProvider->acquireControls` (slot +0x08) inside `Car::pollControls`; zeroed by `Car::step` while controls are locked | start of `Car::step`, before `stepComponents` |
| `Car::valueCache.speed` | `Car::stepPreCacheValues` 0x1402768c0 | before any car is stepped |
| `ISuspension::steerTorque` of wheels 0 and 1 | suspension `step` (reset), then `Tyre::step` through hub slots +0x18, +0x20, +0xc0 | `stepComponents` items 3 and 4 |
| `tyres[i].status.angularVelocity`, `load`, `ndSlip`, `flatSpot`, `blister`; `tyres[0].data.angularInertia` | tyre code (`docs/map/tyre.md`); driven wheels' speed by the drivetrain in the previous step | `Tyre::step` of this step |
| `tyres[i].surfaceDef` (`vibrationGain`, `vibrationLength`) | `Tyre::step` ray cast of the previous step | used by `pollControls` |
| `Car::accG` | `Car::step` of the previous step | used by `pollControls` |
| `PhysicsEngine::gyroWheelGain`, `flatSpotFFGain`, `mzLowSpeedReduction`, `damperMinValue`, `damperGain` | `PhysicsEngine` constructor | once |
| `provider->ffFilter`, `useFakeUndersteerFF` | `DICarControl` constructor (0 / false for every other provider) | once |
| `Car::userFFGain` | `CarAvatar::setUserFFGain` / `modifyUserFFGain` (queued onto the physics thread), setup item `FF_GAIN` | on user action |
| `Car::steerAssist` | car.ini, setup item `STEER_ASSIST`, console variable `steerAssist` | load / on user action |
| `Car::ffMult` | car.ini; `CarAvatar::setFFMult` 0x1400da160 (console variable `ffMult`, FFB app) | load / on user action |
| Suspension damage, toe (`toeOUT_Linear`) | collision callback, `SetupManager` | take effect at the next `setSteerLengthOffset`: the next step for the front wheels; for the rear wheels only on a 4WS car, otherwise only when the rear-toe setup item reseats them (suspension map 5.8) |

### Outputs (who reads what this system writes)

| What | Read by |
|---|---|
| `Car::finalSteerAngleSignal` | `SteeringSystem::step`; `DynamicController::getInput` (`WHEEL_STEER_DEG`), so any `ctrl_*.ini` controller can use it |
| Rod offset → `carSteer` anchor of the steering joint | the rigid-body solver in `core->step` of the same step; the tyres see the new wheel heading in the next step |
| `Car::lastFF` | `Car::getFinalFF`, `Car::getPhysicsState` (`lastFF_Pure`), the provider through `sendFF` |
| `Car::getFinalFF()` | `SharedMemoryWriter::updatePhysics` 0x140186ef0 → shared-memory field `finalFF`; the same product is `CarPhysicsState::lastFF_Final` |
| `Car::lastPureMZFF`, `Car::lastGyroFF` | telemetry channels `FF_PURE` and `FF_GYRO` (`Telemetry::init` 0x1402bc010, in the `else` branch of `debugPhysics == false`, i.e. only with `[TELEMETRY] DEBUG_PHYSICS=1`) |
| `steerLock · controls.steer` (degrees) | `Telemetry::step` 0x1402bf5b0 (`STEERANGLE`), `CarPhysicsState::steer` (graphics steering wheel, net code), controllers (`STEER_DEG`) |
| `Car::steerLock` | the provider (`CarControlsInput::steerLock`); `AIDriver::stepSteer` 0x1402a2630 (`(slip angle in degrees ∓ 10) · 12 / steerLock + oldSteer` above 30 km/h; what the two results bound is the AI atlas' business); `ACClient::sendCarPosition` 0x140052230 (net); graphics steering wheel through `CarPhysicsInfo::steerLock` |
| `controls.steer` (-1..1) | shared-memory field `steerAngle` (`SharedMemoryWriter::updatePhysics`; field identified by its position in the block: packetId, gas, brake, fuel, gear, rpms, steerAngle) |
| `VibrationDef`, rpm | the provider (`setVibrations` +0x48, `setEngineRPM` +0x50) |

### `ICarControlsProvider` slots (vtable 0x1404b2030; names from the `DICarControl` vtable 0x1404b2098)

| Slot | Method | Used by |
|---|---|---|
| +0x08 | `acquireControls(CarControls*, float dt, CarControlsInput*)` | `Car::pollControls` |
| +0x10 | `getAction(int)` | `Car::step` (headlight switch) |
| +0x18 | `sendFF(float ff, float damper, float userGain)` | `Car::onTyresStepCompleted`; `Car::pollControls` sends zeros while locked |
| +0x20 | `getName()` | not used by the step |
| +0x28 | `getFFGlobalGain()` | `Car::getFinalFF`, `Car::getPhysicsState` |
| +0x38 | `onAutoShifterChanged` | not steering |
| +0x48 | `setVibrations(VibrationDef*)` | `Car::pollControls` |
| +0x50 | `setEngineRPM(float rpm, float limit)` | `Car::pollControls` |

**Where the hardware side starts:** `DICarControl::sendFF` 0x140080e40 is the function that
receives the physics value, on the physics thread, 333 times a second. Everything from there on
(`InputDevice::sendFF` 0x14023f9b0, DirectInput effects) is the input atlas.

### Position in `Car::stepComponents`

Suspensions (#3) → tyres (#4, force feedback computed and sent after the fourth tyre) → ... →
`SteeringSystem::step` (#10) → drivetrain → ... → ODE solve. Consequences:
- The tyre forces of a step are computed with the wheel heading left by the previous solve.
- The steering input read at the start of a step reaches the rod anchors in that same step and
  the wheels in that step's solve; the tyres react one step later.
- The force feedback of a step is built from this step's tyre forces.

---

## 7. Port notes

**Effort: S.** The physics part is five small routines of plain float arithmetic (the angle
formula, `SteeringSystem::step`, `Car::getSteerFF`, `Car::onTyresStepCompleted`, the vibration
block of `pollControls`) plus one ini section. The wheel side (`DICarControl`, `FFPostProcessor`)
is separate and belongs to the input layer: S on its own, but not needed for a headless car.

**Port before it:**
- Suspension steer bookkeeping: `setSteerLengthOffset`, `getSteerBasis`, `addForceAtPos` /
  `addTorque` / `addLocalForceAndTorque` with the `steerTorque` sum (suspension map), and the
  distance-joint reseat in the rigid-body layer (physics engine map).
- Tyres (done).
- `DynamicController` + `Curve` only for the two 4WS cars; ks_ferrari_f2004 needs neither.
- A controls-provider trait with the eight slots above. A recording provider (stores `sendFF`
  arguments) is enough for tests.

**Bit-exact traps:**
- Keep the operation order of 5.5 exactly, including the grouping
  `((Δ·333.33334)/|ratio|) · ((|ω1|+|ω0|)·I) · gain` and `st = st1 + st0`.
- The flat-spot and blister maxima and the `· 0.003` are done in double, then converted to float.
- `powf` from MSVCR120 (same library call the tyre port already had to match) for steer assist.
- `(float)(int)(x/p)` in `ksSawToothWave` truncates toward zero; `flatSpotPhase` is never wrapped,
  so its float precision degrades over a long session exactly as in the game.
- `steerAssist == 1.0` selects the no-`powf` branch; keep the branch, do not call `powf(x, 1)`.
- The `+ 1.0` in the flat-spot term must be kept.
- `_fdtest` guard on the steer angle: inf and NaN become 0.

**Oracle, same style as the tyre one** (call the game's own functions on prepared memory):

| Function | Needs | Compare |
|---|---|---|
| `Car::getSteerFF` 0x140272180 | A zeroed `Car`-sized block (0x3ea0) with: `suspensions` vector of two fake objects whose vtable slot +0x30 returns a chosen float; `controlsProvider` → 0x18-byte fake (`ffFilter` +0xc, `useFakeUndersteerFF` +0x12); `ksPhysics` → fake with `flatSpotFFGain` +0x138 and `gyroWheelGain` +0x148; `mzCurrent`, `steerLock`, `steerRatio`, `controls.steer`, `lastSteerPosition`, `flatSpotPhase`, `steerAssist`, `ffMult`; in the four embedded `Tyre`s only `status.angularVelocity`, `load`, `ndSlip`, `flatSpot`, `blister` and `tyres[0].data.angularInertia`. No other object is touched. | return value, `mzCurrent`, `lastSteerPosition`, `lastPureMZFF`, `lastGyroFF`, `flatSpotPhase`, as bit patterns, over sequences of calls |
| `Car::onTyresStepCompleted` 0x140274cd0 | the same block plus `valueCache.speed`, `isControlsLocked`, `blackFlagged`, `userFFGain`, `mzLowSpeedReduction`, `damperMinValue`, `damperGain`, and a provider vtable whose slot +0x18 records its three float arguments | `lastFF` and the recorded `(ff, damper, gain)` |
| `SteeringSystem::step` 0x1402b81b0 | a `SteeringSystem` with `car` → block holding `finalSteerAngleSignal` and a `suspensions` vector of four fakes whose slot +0x28 records the float. For 4WS build `ctrl4ws` with the game's own `DynamicController::DynamicController` 0x1402af330 on a real `ctrl_4ws.ini` and fill the inputs `getInput` reads | the four recorded offsets over a sequence of steps (the controller has state) |
| `FFPostProcessor::getProcessedFF` 0x140279a80 | the 0x90 struct; for `Lut` a `Curve` built by the game's own curve code | return value |
| `ksSawToothWave` 0x14022d2a0, `DynamicController::eval` 0x1402b0c00 | plain arguments / a built controller | return value |

The steer-angle formula is inline in `Car::step` and has no function of its own; test it by
formula, or through the whole-car oracle (`docs/map/car_step.md`).

**Check against the recorded lap:** with an AI driver `ffFilter` is 0, `useFakeUndersteerFF` is
false, `getFFGlobalGain()` is 1.0, and `userFFGain` is 1.0 unless `cfg/user_ff.ini` has an entry for the
car or the gain keys were used, so the shared-memory `finalFF` then equals `lastFF` exactly. That makes `finalFF` a direct probe of the summed front steer
torque of a whole Rust car (through the `− 1.0` offset above 7 rad/s, `FFMULT`, the 0.86 exponent
of the F2004 and the low-speed fade), and `steerAngle` gives back `controls.steer`.

---

## 8. Open questions

1. **Sign convention of steering.** That positive `controls.steer` means "right", and that the
   negative `STEER_RATIO` of 19 cars compensates a steering arm on the other side of the axis, is
   interpretation from the data, not traced through the geometry. The formula itself is confirmed.
2. **`INIReader::getFloat` on a missing key** (closed by the reviewer, confirmed):
   `INIReader::getString` 0x1402360f0 calls `INIReader::errorReport` 0x140234e70 and returns an
   empty string; `INIReader::getFloat` 0x1402358c0 returns 0.0 for an empty string, in verbose mode
   too. `errorReport` passes the text to the static error handler when one is installed and calls
   `ksGenerateCrash` only when the static `INIReader::crashAtError` is set (written in `wWinMain`
   and `acLocaleSetLocale`; which ini switch feeds it was not followed). So "missing → 0" holds
   whenever the game keeps running.
3. **The `+ 1.0` in the flat-spot term** is transcribed from the disassembly
   (`addss xmm0, xmm8` with xmm8 = 1.0 at 0x1402723b3). Whether it was intended is unknown.
4. **`physics.ini`.** "Not read" rests on the strings being absent from `acs.exe`. Another module
   (launcher, a DLL) could read the file, but nothing in `acs.exe` writes `flatSpotFFGain` except
   the constructor (searched by member name in the pseudo-C, not by raw offset).
5. **`FFPostProcessor::type` is not initialised** by the constructor. With `ENABLED=1` and a
   `TYPE` that is neither `GAMMA` nor `LUT` the behaviour depends on whatever is in memory.
6. **Centre boost** in `DICarControl::sendFF` multiplies the force by
   `(1 − |axis|/range)·gain` inside the range, which scales rather than boosts. Transcribed from
   the disassembly; the meaning of the axis value (slot +0x10 of `DIControlAxis`) was not opened.
7. **Curb vibration when no surface has both a gain and a length:** the mean length is then 0/0.
   The comparison `!= 0` on a NaN was not followed in the disassembly. Hardware-only effect.
8. **`userFFGain` clamp 0..200** in `CarAvatar::modifyUserFFGain` while the value is used as a
   plain multiplier (1.0 = 100 %): transcribed, looks like a percent/fraction mix-up, harmless.
9. **`torqueModeEx` is taken as always `original`.** Based on a search of all pseudo-C for the
   member name and for its offset. If something outside `acs.exe` (a patch or extension) sets it,
   the two dead rows of 5.4 come alive: brake torque would then leak into the force feedback
   through the steering-axis component of the wheel axle direction, and a multilink front axle
   would lose the grip-force part (its `addLocalForceAndTorque` books no steer torque).
10. **Rear-steer sign and geometry.** Both rear wheels get the same offset, so they turn the same
    way; which way relative to the fronts depends on the rear rod geometry and the LUT signs
    (opposite signs in the two shipped files). Not worked through.
11. **Telemetry gating.** `FF_PURE` / `FF_GYRO` sit in the branch taken when
    `[TELEMETRY] DEBUG_PHYSICS` is on; read from the block structure of the pseudo-C of
    `Telemetry::init` (lines 616 to 658 of the saved file), not from the disassembly.
12. **Choices made here:** the wheel/DirectInput side is summarised (5.8) but not mapped: no
    `DICarControl::acquireControls` maths beyond what feeds physics, no `InputDevice` internals.
    `DynamicController` is referenced from the brakes map instead of being repeated in full.
    `Car::pollControls` vibrations are given in short form because `docs/map/car_step.md` 5.4
    covers the function.

---

## 9. Direct answers to the questions asked for this system

| Question | Answer |
|---|---|
| Is Ackermann a parameter? | No. Both front rods get the same offset; Ackermann, bump steer and the real steering ratio come from the pick-up points. |
| Is there power steering? | No. `STEER_ASSIST` is only an exponent on the force-feedback value; it does not touch the car. |
| Gyro option in physics? | Yes: `[FF_EXPERIMENTAL] ENABLE_GYRO` sets `gyroWheelGain` to 0.004 (else 0). It is a force-feedback term only, no torque is applied to the car. |
| Damper in physics? | The strength is computed in physics (`Car::onTyresStepCompleted`) from speed, `DAMPER_MIN_LEVEL` and `DAMPER_GAIN`, and passed as the second `sendFF` argument. The effect itself is a DirectInput damper. |
| Minimum force in physics? | No. `MIN_FF` is applied in `DICarControl::sendFF`. |
| Low-speed handling? | `[LOW_SPEED_FF]` fade in physics (5.6), plus the damper rising below 10 km/h. |
| Gamma / LUT in physics? | No. `FFPostProcessor` runs in `DICarControl::sendFF`, after the gains, on the wheel only. |
| `physics.ini` settings? | The file is not read by `acs.exe`; the flat-spot gain is the constructor's 0.05. |
| What is telemetry `finalFF`? | `getFFGlobalGain() · lastFF · userFFGain` (5.9): the physics value with the two gains, without the wheel-side extras. |
| What reaches the hardware? | `clamp(post((lastFF' + vibration) · ffGain · userFFGain) · 10000, ±10000)` as a DirectInput constant force plus the damper coefficient, where `lastFF'` has centre boost and minimum force applied (5.8). |
