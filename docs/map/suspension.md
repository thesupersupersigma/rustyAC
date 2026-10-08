# Suspension

Source: `acs.exe` + `acs.pdb`, read through the local index only: Ghidra pseudo-C in `re/decomp/`
(folders `Suspension`, `SuspensionStrut`, `SuspensionAxle`, `SuspensionML`, `Damper`, `HeaveSpring`,
`AntirollBar`, `ActiveActuator`, plus the suspension parts of `Car`, `SteeringSystem`,
`SetupManager`), struct layouts in `re/types/`, `tools/re_query.py`, `tools/disasm.py` and
`tools/ini_survey.py` over the 113 cars in `cardata/`. Raw copies of everything cited:
`re/car/suspension/`. Nothing in the game folder, the Ghidra project or git was changed. No Rust was
written. "Confirmed" below means seen in the pseudo-C or disassembly; anything else is marked as
interpretation or listed in section 8.

Offsets of the `ISuspension` base members (`k`, `rodLength`, ...) are shown by Ghidra as
`_padding_`; they were resolved from the disassembly (`Suspension::step`, the constructors, the four
`step` functions and `SetupManager::initItems`).

---

## 1. Plain-English summary

Every wheel has its own small rigid body, the hub, which carries the tyre.
The hub is tied to the car body by rods of fixed length: five for a double wishbone (four wishbone
arms and the steering rod), three plus a sliding strut for a strut suspension. A rigid rear axle is
one single body for both rear wheels, held by a handful of rods (four in every car that has one).
The rods only fix the geometry. They do not carry the car: on their own the hub could still swing
up and down freely.
What holds the car up is added by the game each physics step (333 times a second). For every wheel
it measures how far the hub has moved up relative to the body, turns that into a spring force, and
pushes the hub down and the body up by the same amount.
It then measures how fast the hub is moving up or down relative to the body and adds a damper
force that opposes the motion, with separate strengths for slow and fast movement and for
compression and extension.
If the hub travels too far it hits a bump stop (a very stiff extra spring) at the top or bottom, and
past the "packer" distance an extra stiff spring is added on top of the main one.
After the tyres have run, an optional third spring per axle (heave spring, Formula cars) acts on the
average height of the left and right hub (7 of the 113 cars have one), and later in the step the
anti-roll bar pushes the left and right hubs toward the same height.
Steering does not rotate the wheel directly: it slides the body-side end of the steering rod
sideways, and the rod then drags the hub round. Toe is the same trick with a fixed offset, and a
bent suspension after a crash is again the same offset.
The twist the tyre forces produce about the steering axis is added up every step and is what the
steering wheel force feedback is built from.

---

## 2. Classes

| Class | Size | What it is for |
|---|---|---|
| `ISuspension` | 0x38 | Interface + shared numbers: `k` +0x08, `progressiveK` +0x0c, `bumpStopRate` +0x10, `bumpStopProgressive` +0x14, `staticCamber` +0x18, `bumpStopUp` +0x1c, `bumpStopDn` +0x20, `rodLength` +0x24, `toeOUT_Linear` +0x28, `packerRange` +0x2c, `baseCFM` +0x30. 25 vtable slots (section 9). |
| `Suspension` | 0x210 | Double wishbone ("DWB"). Owns the hub body, 5 distance joints, a `Damper`, an `ActiveActuator`, damage data. |
| `SuspensionStrut` | 0x1e8 | Strut suspension ("STRUT"). Hub body + small strut body, 3 distance joints, 1 slider joint, 1 ball joint. |
| `SuspensionAxle` | 0xb0 | One side of a rigid rear axle ("AXLE"). Two instances (Left, Right) share one body, `Car::rigidAxle`. |
| `SuspensionML` | 0xc8 | Multilink ("ML"): hub + 5 free distance joints read as `JOINT0..4`. Fully implemented but used by 0 of the 113 cars. Four base members (`bumpStopRate`, `bumpStopUp`, `bumpStopDn`, `packerRange`) are never initialised, and its step reads two of them (section 5.5). |
| `SDWSuspensionData` | 0x7c | DWB pick-up points (`carTopWB_F/R`, `carBottomWB_F/R`, `tyreTopWB`, `tyreBottomWB`, `carSteer`, `tyreSteer`, `refPoint`), `hubMass`, `hubInertiaBox`. Held twice: `dataRelToWheel` (as in the ini) and `dataRelToBody`. |
| `SStrutSuspensionData` | 0x70 | Same for the strut (`carStrut`, `tyreStrut`, `carBottomWB_F/R`, `tyreBottomWB`, `carSteer`, `tyreSteer`, `refPoint`, `hubMass`, `hubInertiaBox`). |
| `AxleJoint` (0x40) / `AxleBall` (0x20) | | One axle link: two ends, each with `relToAxle`, `relToCar`, `joint`. |
| `MLJoint` (0x38) / `MLBall` (0x18) | | One multilink rod: `ballCar`, `ballTyre` (`relToTyre`, `relToCar`), `joint`. |
| `SuspensionStatus` | 0x8 | Output: `travel` (m), `damperSpeedMS` (m/s). |
| `Damper` | 0x18 | `reboundSlow`, `reboundFast`, `bumpSlow`, `bumpFast`, `fastThresholdBump`, `fastThresholdRebound`. One method, `getForce`. |
| `SusDamageDef` / `SusStrutDamageDef` | 0x1c | `damageAmount`, `damageDirection` (+1 or -1, random: `rand()·3.051851e-05·100 ≥ 50 → +1`, else −1, drawn once in the constructor of a DWB, a STRUT and an ML wheel, before `loadINI`; `SuspensionAxle` draws none, so a car with DWB front and AXLE rear draws 2 numbers, not 4), `minVelocity`, `damageGain`, `maxDamage`, `isDebug`, `lastAmount`. |
| `HeaveSpring` | 0x58 | Third spring of one axle: `k`, `progressiveK`, `rodLength`, `packerRange`, `bumpStopRate`, `bumpStopUp/Dn`, a `Damper`, two `Suspension*`. `Car::heaveSprings[2]`. Only initialised when all four wheels are DWB (section 5.6). |
| `HeaveSpringStatus` | 0x4 | `travel`. |
| `AntirollBar` | 0x48 | `carBody`, `hubs[2]`, stiffness `k`, optional `DynamicController ctrl`. `Car::antirollBars[2]`. |
| `ActiveActuator` | 0x18 | `targetTravel` + `PIDController` (0x14: `P`, `I`, `D`, `currentError`, `integral`). Only reachable through `Suspension::useActiveActuator`, which nothing sets (section 8). |
| `SuspensionType` (enum) | 4 | `DoubleWishbone` = 0, `Strut` = 1, `Axle` = 2, `Multilink` = 3 (values from the stores in `Car::Car`). `Car::suspensionTypeF`, `Car::suspensionTypeR`. |

Car members: `Car::suspensions` (vector of 4 `ISuspension*`, order LF, RF, LR, RR), `Car::rigidAxle`,
`Car::antirollBars`, `Car::heaveSprings`, `Car::axleTorqueReaction`.

Not physics (listed only): `ACSuspension` (wireframe rendering), `SuspensionAvatar`,
`SuspensionGraphicsGenerator`, `CarAvatar::getPackerRange` 0x1400d3540,
`CarAvatar::getSuspensionDebugLines` 0x1400d37d0.

A fifth class implements `ISuspension`: `TyreTester` (the developer tyre test rig, vtable
0x141416b70). Only `getHubWorldMatrix` 0x14044f160, `getPointVelocity` 0x14044f250, `getStatus`,
`getVelocity`, `getMass` and `getDebugLines` have bodies; every force / steer / damage / step slot is
the shared empty function or `return 0`. It is never created by `Car::Car`.

---

## 3. Key functions

### Loading / init

```
Car::Car                                   0x14026bf00
├─ reads [REAR] TYPE; if "AXLE": creates Car::rigidAxle body, reads [AXLE] TORQUE_REACTION
├─ for wheel 0..3: reads [FRONT] TYPE (wheels 0,1) or [REAR] TYPE (wheels 2,3)
│  ├─ "STRUT" → SuspensionStrut::SuspensionStrut   0x1402c38d0
│  │     ├─ SuspensionStrut::loadINI               0x1402c4ee0
│  │     ├─ creates hub + strutBody, setMassBox
│  │     ├─ SuspensionStrut::attach                0x1402c4100  (→ SuspensionStrut::setPositions 0x1402c6210)
│  │     └─ SuspensionStrut::setSteerLengthOffset(0)  0x1402c6540
│  ├─ "DWB"   → Suspension::Suspension             0x1402c0720
│  │     ├─ Suspension::loadINI                    0x1402c1d90
│  │     ├─ creates hub, setMassBox
│  │     ├─ Suspension::attach                     0x1402c0fb0
│  │     ├─ Suspension::setSteerLengthOffset(0)    0x1402c3290
│  │     └─ each of the 5 joints: setERPCFM(0.3, baseCFM)
│  ├─ "ML"    → SuspensionML::SuspensionML         0x1402c8e20
│  │     └─ SuspensionML::loadINI                  0x1402c9970  (creates hub and joints itself;
│  │           places the hub with SuspensionML::setPositions 0x1402ca9d0, same body as ML attach)
│  ├─ "AXLE" and wheel ≥ 2 → SuspensionAxle::SuspensionAxle 0x1402c6b90 (Left for wheel 2, Right for wheel 3)
│  │     └─ SuspensionAxle::setPositions           0x1402c86e0, joints, SuspensionAxle::setERPCFM 0x1402c8680
│  ├─ anything else → printf("ERROR: Cannot create suspension type: %S  for wheel:%d") and exit(1)
│  └─ Tyre::init(tyre[i], suspension[i], ...)      0x140280650
├─ Car::initHeaveSprings                   0x140273f40
│  ├─ dynamic_cast of all 4 suspensions to Suspension (DWB); if any fails: printf
│  │     "IGNORING HEAVE SPRING BECAUSE SUSPENSION %d  IS NOT DOUBLE WISHBONE" and return (no init at all)
│  └─ HeaveSpring::init ×2                 0x1402b3310 → HeaveSpring::initData 0x1402b3340
├─ ... (aero, drivetrain, steering) ...
└─ Car::buildARBS                          0x14026f750
   └─ AntirollBar::init ×2                 0x1402bb610, [ARB] FRONT/REAR, optional ctrl_arb_*.ini
SetupManager::initItems                    0x140289570   registers pointers into the members (section 6)
```

### Every physics step

```
PhysicsEngine::step                        0x140264760
└─ Car::step                               0x140275da0
   ├─ (only if Car::physicsGUID == 0) each suspension->setERPCFM   slot +0xb8
   │      speed² ≥ 1:  (0.3, suspension.baseCFM)      else: (0.9, 1e-7)
   └─ Car::stepComponents                  0x1402764d0
      ├─ BrakeSystem::step, EDL::step
      ├─ each suspension->step(dt)         slot +0xb0
      │     Suspension::step               0x1402c3390
      │     SuspensionStrut::step          0x1402c6600
      │     SuspensionAxle::step           0x1402c8770
      │     SuspensionML::step             0x1402cab00
      │        └─ Damper::getForce         0x1402b3280   (ActiveActuator::eval 0x1402cb610 → PIDController::eval 0x14044e720 on the dead active path)
      ├─ Tyre::step ×4                     calls back into slots +0x08 +0x10 +0x18 +0x20 +0x38 +0xc0
      ├─ HeaveSpring::step ×2              0x1402b3960   (only if heave.k != 0)
      ├─ DRS, AeroMap, Kers, ERS
      ├─ SteeringSystem::step              0x1402b81b0   → suspension->setSteerLengthOffset  slot +0x28
      ├─ AutoBlip, AutoShifter, GearChanger, Drivetrain::step
      ├─ AntirollBar::step ×2              0x1402bb640
      └─ ...
```

Outside the step: `Car::getSteerFF` 0x140272180 (reads `getSteerTorque` of wheels 0 and 1),
`Car::onCollisionCallBack` 0x140274650 (`setDamage`), `Car::resetSuspensionDamageLevel` 0x140275970
(`resetDamage`), `Car::forcePosition` 0x14026fe10 (`stop` + `attach`), `Car::forceRotation`
0x140270040 (`attach`), `Car::getPhysicsState` 0x140270d70 (`getHubWorldMatrix`, `getStatus`,
`getDamage`, `getVelocity`), and the two setup lambdas 0x140288f00 / 0x140288ee0 that call
`setSteerLengthOffset(0)` on the rear wheels when the rear toe setup item changes (section 5.8).

---

## 4. Data it reads

All from `suspensions.ini` of the car (present in 113 of 113 cars), opened separately by each
loader. The per-wheel section is `[FRONT]` for wheels 0 and 1, `[REAR]` for wheels 2 and 3.

### Which TYPE maps to which class, and how many cars use it

| TYPE | Class | Front (of 113) | Rear (of 113) |
|---|---|---|---|
| `DWB` | `Suspension` | 83 | 103 |
| `STRUT` | `SuspensionStrut` | 29 | 5 |
| `AXLE` | `SuspensionAxle` (rear only) | 0 | 4 (`ks_alfa_romeo_gta`, `ks_ferrari_250_gto`, `ks_maserati_250f_12cyl`, `ks_maserati_250f_6cyl`) |
| `ML` | `SuspensionML` | 0 | 0 |
| `COSMIC` | none | 1 | 1 (`vrc_formula_alpha_2026_csp`) |

Unknown type (confirmed in `Car::Car`): the string is compared with `STRUT`, `DWB`, `ML`, `AXLE` in
that order; if no object was created the loop breaks, the game prints
`ERROR: Cannot create suspension type: <type>  for wheel:<n>` and calls `exit(1)`. `AXLE` on a front
wheel ends the same way. So `COSMIC` is not something the stock exe supports; that car is meant for a
modded game. The same file also carries `[_EXTENSION]`, `[ARB] EXTEND / *_MOTION_RATIO`,
`STEER_JOINT_0_LENGTH_OFFSET`, `BODY_0_*`, none of which appear as strings in the exe.

### `[HEADER]`, `[BASIC]`, wheel position

| Key | Goes to | Notes |
|---|---|---|
| `[HEADER] VERSION` (int) | local | 2 (62 cars), 1 (45), 4 (6). Switches: DWB/STRUT `VERSION ≥ 2` → read `RIM_OFFSET`; AXLE `VERSION ≥ 3` → `LEAF_SPRING_LAT_K`, `VERSION ≥ 4` → `ATTACH_REL_POS`. ML reads it and ignores it. |
| `[BASIC] WHEELBASE` (m), `CG_LOCATION` (front weight share 0..1) | `refPoint.z` | front wheels `z = (1 − CG_LOCATION)·WHEELBASE`, rear wheels `z = −(CG_LOCATION·WHEELBASE)` |
| `[FRONT]/[REAR] TRACK` (m) | `refPoint.x` | wheel 0 and 2: `+TRACK·0.5`; wheel 1 and 3: `−(TRACK·0.5)` |
| `[FRONT]/[REAR] BASEY` (m) | `refPoint.y` | height of the wheel centre relative to the body origin (the centre of gravity) |

`refPoint` is the design position of the wheel centre in body coordinates. It is stored as
`dataRelToWheel.refPoint` (DWB, STRUT), `basePosition` (ML) or `axleBasePos = (0, BASEY, −CG·WB)` +
`track = TRACK·0.5` + `referenceY = BASEY` (AXLE).

### Per-wheel keys (`[FRONT]` / `[REAR]`)

| Key | Member | Used by | Default / conversion |
|---|---|---|---|
| `TYPE` | chooses the class | `Car::Car` | see above |
| `HUB_MASS` (kg) | `hubMass` | all | DWB, STRUT: `≤ 0 → 20`. STRUT splits it: hub 80 %, strut body 20 %. AXLE: `[REAR] HUB_MASS` is the mass of the whole axle body. ML: used as is. |
| `WBCAR_TOP_FRONT`, `WBCAR_TOP_REAR`, `WBCAR_BOTTOM_FRONT`, `WBCAR_BOTTOM_REAR`, `WBTYRE_TOP`, `WBTYRE_BOTTOM`, `WBCAR_STEER`, `WBTYRE_STEER` (x,y,z in m, relative to the wheel centre) | `dataRelToWheel.carTopWB_F`, `.carTopWB_R`, `.carBottomWB_F`, `.carBottomWB_R`, `.tyreTopWB`, `.tyreBottomWB`, `.carSteer`, `.tyreSteer` | DWB (all 8). STRUT reads the bottom, steer ones only. | Written for the wheel on the negative-x side (wheels 1 and 3, i.e. the right side given the LF, RF, LR, RR order), where +x points inboard. For the wheel with `refPoint.x > 0` every x is negated after loading. |
| `STRUT_CAR`, `STRUT_TYRE` | `dataRelToWheel.carStrut`, `.tyreStrut` | STRUT | same mirroring |
| `JOINT0_CAR` … `JOINT4_CAR`, `JOINT0_TYRE` … `JOINT4_TYRE` | `joints[i].ballCar.relToTyre`, `.ballTyre.relToTyre` | ML | always 5; x negated when `basePosition.x > 0`. Joint 4 is the steering rod. |
| `RIM_OFFSET` (m) | added as `−RIM_OFFSET` to the x of every pick-up point (all 8 for DWB, all 7 for STRUT; done inside `loadINI`, i.e. before the mirroring, and skipped when the value is 0) | DWB, STRUT, only if `VERSION ≥ 2` | 68 cars have the key |
| `SPRING_RATE` (N/m) | `k` | all | constructor default 90000 (DWB, STRUT) is always overwritten by the ini read |
| `PROGRESSIVE_SPRING_RATE` (N/m per m) | `progressiveK` | all | 0 in 73 (front) / 82 (rear) cars |
| `ROD_LENGTH` (m) | `rodLength` | all | adds to travel: positive raises the car |
| `BUMPSTOP_UP` (m) | `bumpStopUp` | DWB, STRUT, AXLE | not read by ML |
| `BUMPSTOP_DN` (m) | `bumpStopDn = −value` | DWB, STRUT, AXLE | |
| `BUMP_STOP_RATE` (N/m) | `bumpStopRate` | DWB, STRUT, AXLE (read) | `0 → 500000`. Not read by ML. |
| `BUMP_STOP_PROGRESSIVE` | `bumpStopProgressive` | DWB only, only if the key exists | default 0; 1 car has the key (value 0) |
| `PACKER_RANGE` (m) | `packerRange` | DWB, STRUT | 0 disables. Not read by AXLE or ML. |
| `DAMP_BUMP`, `DAMP_REBOUND` (N·s/m) | `damper.bumpSlow`, `damper.reboundSlow` | all | |
| `DAMP_FAST_BUMP`, `DAMP_FAST_REBOUND` | `damper.bumpFast`, `damper.reboundFast` | all | `0 →` the slow value (DWB, STRUT, AXLE, heave). **ML has no fall-back**: `SuspensionML::loadINI` stores the raw value. |
| `DAMP_FAST_BUMPTHRESHOLD`, `DAMP_FAST_REBOUNDTHRESHOLD` (m/s) | `damper.fastThresholdBump`, `.fastThresholdRebound` | all | `0 → 0.2` (DWB, STRUT, AXLE, heave). **ML has no fall-back**, so a missing key leaves the threshold at 0 and every speed takes the "fast" branch. |
| `STATIC_CAMBER` (deg) | `staticCamber = −(value · 0.017453)`, then negated again for odd wheel index (1, 3) | DWB, STRUT, ML | constant is the float `0x3c8ef998`. Not read by AXLE. |
| `TOE_OUT` (m of steering-rod shift) | `toeOUT_Linear` | all (AXLE reads it but has no steering rod) | |

`Damper::Damper` 0x1402b3250 defaults (5000, 300, 2000, 300, 0.2, 0.2) are overwritten by the reads.
No key sets `hubInertiaBox`; it stays zero and the constructors then use the box `(0.2, 0.6, 0.6)` m
for the hub inertia (`setMassBox`).

Base members that are **not** initialised by a constructor (reviewer, from the stores in the four
constructors): the inlined `ISuspension` constructor only writes `bumpStopProgressive = 0` and
`baseCFM = 1e-7`. DWB and STRUT add `k = 90000`, `staticCamber = 0`, `rodLength = 0`,
`toeOUT_Linear = 0` and their `loadINI` writes every other base member, so nothing is left open
there. AXLE never writes `staticCamber` (+0x18) or `packerRange` (+0x2c): both are heap garbage,
and neither is read by the axle code (its `getPackerRange` is the `return 0` stub). The setup items
`CAMBER_LR/RR` and `PACKER_RANGE_LR/RR` point at them, so only the setup screen shows the garbage.
The port has 0 for both and does not compare these two values with the game. ML never writes
`bumpStopRate`, `bumpStopUp`, `bumpStopDn`, `packerRange`; two of them are read by its step
(section 5.5).

### `[AXLE]` (4 cars)

| Key | Member | Notes |
|---|---|---|
| `TORQUE_REACTION` | `Car::axleTorqueReaction` | read in `Car::Car`; used by the drivetrain (section 6) |
| `LINK_COUNT` (int) | number of links | 4 in all four cars |
| `J<i>_CAR`, `J<i>_AXLE` (x,y,z) | `joints[i].ballCar.relToAxle`, `joints[i].ballAxle.relToAxle` | axle-body coordinates. Three cars also define `J4_*`, which is never read because `LINK_COUNT=4`. |
| `ATTACH_REL_POS` | `attachRelativePos` | `VERSION ≥ 4`; default 1.0. Where along the half-track the spring sits. |
| `LEAF_SPRING_LAT_K` (N/m) | `leafSpringK.x` | `VERSION ≥ 3`; 350000 in 3 cars, 0 in 1 |

### `[ARB]`, `[HEAVE_FRONT]`, `[HEAVE_REAR]`, `[DAMAGE]`

| Section / key | Member | Notes |
|---|---|---|
| `[ARB] FRONT`, `REAR` (N/m) | `antirollBars[0].k`, `antirollBars[1].k` | 113 cars. Read in `Car::buildARBS`. |
| `ctrl_arb_front.ini`, `ctrl_arb_rear.ini` (files) | `antirollBars[i].ctrl` | 2 cars. If present, a `DynamicController` overwrites `k` every step. |
| `[HEAVE_FRONT]` / `[HEAVE_REAR]` | `heaveSprings[0]` / `[1]` | 7 / 6 cars (all of them DWB front and rear). Read only when all four wheels are DWB (`Car::initHeaveSprings`); otherwise the sections are ignored. Section present → `isPresent = true`. Keys: `ROD_LENGTH`, `SPRING_RATE`, `PROGRESSIVE_SPRING_RATE`, `BUMPSTOP_UP`, `BUMPSTOP_DN` (negated), `BUMP_STOP_RATE` (`0 → 500000`), `PACKER_RANGE`, the six `DAMP_*` keys with the same fall-backs as a wheel. Path used: `content/cars/<unixName>/data/suspensions.ini`. |
| `[DAMAGE] MIN_VELOCITY`, `GAIN`, `MAX_DAMAGE`, `DEBUG_LOG` | `damageData.minVelocity`, `.damageGain`, `.maxDamage`, `.isDebug` | 110 cars. Read by DWB and STRUT only if the section exists; constructor default `minVelocity = 15`, others 0. ML and AXLE never read it. |
| `[RIGIDITY]` | nothing | DWB and AXLE call `hasSection("RIGIDITY")` and throw the answer away. Dead code. `baseCFM` therefore always stays `1e-7`. |
| `[GRAPHICS_OFFSETS] WHEEL_LF … SUSP_RR` | graphics only | read by `SuspensionAvatar::addModel` 0x1401b32b0 and `GhostCar::init3D` 0x140105520, never by the physics classes |

---

## 5. Each step: inputs, maths, outputs

Notation. `B` = `Car::body`, `H` = the hub. `up` = row 2 of B's world matrix (`M21, M22, M23`), the
body's own vertical axis in world space; "row 1" below is `M11, M12, M13`, the body's (or axle's)
own x axis. IRigidBody slots used (names from `RigidBodyODE`'s vtable):
`getWorldMatrix` +0x48, `localToWorld` +0x50, `worldToLocal` +0x58, `getVelocity` +0x78,
`getPosition` +0xa0, `addLocalForceAtLocalPos` +0xf0, `addForceAtLocalPos` +0xf8,
`getLocalPointVelocity` +0x100, `getPointVelocity` +0x108, `addForceAtPos` +0x110, `addTorque`
+0x118. `self.addForceAtPos(F, P)` means the suspension's own slot +0x18 with both bools false.
All arithmetic is 32-bit float.

### 5.1 Damper::getForce(v) 0x1402b3280 (confirmed from disassembly)

`v > 0` selects the bump coefficients. For DWB, AXLE, ML and the heave spring `v > 0` really is
bump (hub moving up relative to the body). **For STRUT the caller's sign is the other way round**
(section 5.3): there `v > 0` means the strut is getting longer.

```
if v > 0:
    if v > fastThresholdBump:  return −((v − fastThresholdBump)·bumpFast + fastThresholdBump·bumpSlow)
    else:                      return −(v·bumpSlow)
else:
    if |v| > fastThresholdRebound: return fastThresholdRebound·reboundSlow − (fastThresholdRebound + v)·reboundFast
    else:                          return −(v·reboundSlow)
```

### 5.2 Suspension::step (DWB) 0x1402c3390 (confirmed from disassembly)

```
steerTorque = 0
hubPos   = H.getPosition()                 hubLocal = B.worldToLocal(hubPos)
dy       = hubLocal.y − refPoint.y         (refPoint = dataRelToWheel.refPoint)
travel   = dy + rodLength                  status.travel = travel

F = (travel·progressiveK + k)·travel
if packerRange != 0 and travel > packerRange and k != 0:
    d = travel − packerRange;  F += (d·bumpStopProgressive + bumpStopRate)·d
if F > 0:
    self.addForceAtPos(−F·up, hubPos)
    B.addLocalForceAtLocalPos((0, F, 0), refPoint)

v = (H.getVelocity() − B.getLocalPointVelocity(refPoint)) · up
status.damperSpeedMS = v
f = Damper::getForce(v)
self.addForceAtPos(f·up, hubPos)
B.addForceAtLocalPos(−f·up, refPoint)

if bumpStopUp != 0 and dy > bumpStopUp and k != 0:
    d = dy − bumpStopUp;  Fb = (d·bumpStopProgressive + bumpStopRate)·d
    self.addForceAtPos(−Fb·up, hubPos);  B.addLocalForceAtLocalPos((0, Fb, 0), hubLocal)
if bumpStopDn != 0 and dy < bumpStopDn and k != 0:
    d = dy − bumpStopDn;  Fb = (d·bumpStopProgressive + bumpStopRate)·d
    self.addForceAtPos(−Fb·up, hubPos);  B.addLocalForceAtLocalPos((0, Fb, 0), hubLocal)
```

Points to note:
- **No motion ratio is computed.** The spring and damper act along the body's vertical axis between
  the hub centre and the fixed body point `refPoint`. `SPRING_RATE` and `DAMP_*` are therefore wheel
  rates, which is what the comments in the example car say ("Do not use spring value but calculate
  wheel rate").
- The spring can only push (`F > 0`); the damper is always applied.
- The hub side of every force is applied along world `up`; the body side of the spring and bump
  stops is given in body-local axes `(0, F, 0)`, the body side of the damper in world axes.
- Bump stops compare `dy`, without `rodLength`; the packer compares `travel`, with it.
- The body side of the bump-stop force is applied at the hub's current position, not at `refPoint`.
- Evaluation order of the damper-speed dot product (matters for bit-exactness; DWB, STRUT, AXLE
  and ML read in the disassembly, heave from the pseudo-C): DWB, STRUT and heave add `d.y·a.y` first, then
  `d.x·a.x`, then `d.z·a.z`; AXLE and ML add x, y, z in order. Force vectors are always
  `axis component · scalar`, one multiply per component.
- If `useActiveActuator` were true the spring/damper block is replaced by
  `f = PID(target 0, measured dy, dt)` applied the same way as the damper force, both
  `car.antirollBars[i].k` are zeroed every step and `status.travel = dy`. Nothing sets the flag.

### 5.3 SuspensionStrut::step 0x1402c6600 (confirmed from disassembly)

```
steerTorque = 0
A = B.localToWorld(dataRelToBody.carStrut)       top mount, world
P = H.localToWorld(dataRelToWheel.tyreStrut)     strut foot on the hub, world
d = P − A;  len2 = d.y·d.y + d.x·d.x + d.z·d.z;  len = sqrt(len2)  (0 if len2 == 0)
dir = d · (1.0/len)  (only if len != 0; one divide, then three multiplies, not three divides)
travel = (strutBaseLength + rodLength) − len      status.travel = travel

F = max((travel·progressiveK + k)·travel, 0)
if packerRange != 0 and travel > packerRange:  F += (travel − packerRange)·bumpStopRate
if F > 0:  self.addForceAtPos(F·dir, P);  B.addForceAtPos(−F·dir, A)

hubLocal = B.worldToLocal(H.getPosition());  dy = hubLocal.y − refPoint.y
if dy > bumpStopUp:  Fb = (dy − bumpStopUp)·500000
    self.addForceAtPos(−Fb·up, H.getPosition());  B.addLocalForceAtLocalPos((0, Fb, 0), hubLocal)
if dy < bumpStopDn:  Fb = (dy − bumpStopDn)·500000   (same two calls)

v = (H.getLocalPointVelocity(tyreStrut) − B.getLocalPointVelocity(carStrut)) · dir
status.damperSpeedMS = v;  f = Damper::getForce(v)
self.addForceAtPos(f·dir, P);  B.addForceAtPos(−f·dir, A)
```

Differences from DWB: spring and damper act **along the strut** (so the strut angle gives a real
motion ratio); the packer term is linear (no progressive part, no `k != 0` test); the bump stops use
a hard-coded 500000 N/m, ignore `BUMP_STOP_RATE` and have no "is it zero" test.
`strutBaseLength` is the strut length at the design position (computed in `attach`).

**Damper sign in the strut (reviewer, confirmed in the disassembly 0x1402c69d1 .. 0x1402c6a45).**
`dir` points from the top mount down to the strut foot, and `v` is (foot velocity − mount
velocity) · `dir`, so `v` is the rate of change of the strut length: **positive when the strut
extends** (rebound), negative when it compresses. That is the opposite of DWB / AXLE / ML / heave,
where positive is compression. `Damper::getForce` is the same function for all, so on a STRUT wheel
`DAMP_BUMP`, `DAMP_FAST_BUMP` and `DAMP_FAST_BUMPTHRESHOLD` act while the strut extends and
`DAMP_REBOUND`, `DAMP_FAST_REBOUND` and `DAMP_FAST_REBOUNDTHRESHOLD` act while it compresses, and
`status.damperSpeedMS` has the opposite sign too. The force is still dissipative (for `v > 0`,
`f < 0`, the hub is pulled toward the mount). Whether this swap was intended is interpretation; a
port must reproduce it as written. The spring sign is not affected (`travel` grows in
compression, like DWB). The whole step was re-read in the disassembly by the reviewer: constants
(1.0, 500000.0, −1.0), the `cmovbe` that implements `max(…, 0)`, the y, x, z order of both the
squared length and the damper dot product, and the body-side damper force computed as
`(f·dir_i) · (−1.0)`.

### 5.4 SuspensionAxle::step 0x1402c8770 (confirmed from disassembly)

Runs once for the Left instance and once for the Right; `s = +track` (Left) or `−track` (Right).

```
Am = axle.getWorldMatrix()
P  = Am.translation + Am.row1 · s · attachRelativePos          spring seat on the axle, world
pLocal = B.worldToLocal(P)
base = getBasePosition() = (s, axleBasePos.y, axleBasePos.z)
T  = B.localToWorld((base.x·attachRelativePos, base.y + 0.2, base.z))   body-side point, 0.2 m above
d = T − P;  len = sqrt(d.y·d.y + d.x·d.x + d.z·d.z);  dir = d · (1.0/len)  (only if len != 0)
travel = (0.2 − len) + rodLength                  status.travel = travel

F = −((travel·progressiveK + k)·travel)
if F < 0:  self.addForceAtPos(F·dir, P);  B.addForceAtPos(−F·dir, T)

if leafSpringK.x != 0:
    Fl = (pLocal.x − attachRelativePos·base.x)·leafSpringK.x
    self.addForceAtPos(−Fl·(row 1 of B's matrix), P);  B.addLocalForceAtLocalPos((Fl, 0, 0), pLocal)

dy = pLocal.y − referenceY
if bumpStopUp != 0 and dy > bumpStopUp and k != 0:  Fb = (dy − bumpStopUp)·500000
    self.addForceAtPos(−Fb·up, P);  B.addLocalForceAtLocalPos((0, Fb, 0), pLocal)
if bumpStopDn != 0 and dy < bumpStopDn and k != 0:  Fb = (dy − bumpStopDn)·500000   (same two calls)

v = (self.getVelocity() − B.getPointVelocity(T)) · dir      self.getVelocity (slot +0xa0) = axle.getLocalPointVelocity((s, 0, 0))
status.damperSpeedMS = v;  f = Damper::getForce(v)
self.addForceAtPos(f·dir, P);  B.addForceAtPos(−f·dir, T)
```

`dir` points from the axle up to the body, so the signs are mirrored compared with the strut.
No packer. `BUMP_STOP_RATE` is read but not used. The leaf-spring term is a sideways spring that
keeps the axle centred under the body. All forces go to the shared axle body.
Confirmed in the disassembly of 0x1402c8770 (reviewer): the 0.2 and 500000.0 constants, the
`F < 0` test, the three-part bump-stop tests and the x, y, z order of the damper dot product. Two
details that are easy to miss: the damper speed is measured at the wheel end of the axle
`(s, 0, 0)` while the spring and damper forces are applied at the spring seat `P`
(`s·attachRelativePos`); and `getBasePosition()` is called a second time inside the leaf-spring
branch (same value).

### 5.5 SuspensionML::step 0x1402cab00 (confirmed from disassembly, the whole function)

```
steerTorque = 0
hubPos = H.getPosition();  hubLocal = B.worldToLocal(hubPos)
travel = (hubLocal.y − basePosition.y) + rodLength            status.travel = travel
F = (travel·progressiveK + k)·travel
if packerRange != 0 and travel > packerRange and k != 0:  F += (travel − packerRange)·bumpStopRate
self.addForceAtPos(−F·up, hubPos);  B.addLocalForceAtLocalPos((0, F, 0), basePosition)     (no F > 0 test)
v = (H.getVelocity() − B.getLocalPointVelocity(basePosition)) · up
f = Damper::getForce(v);  status.damperSpeedMS = v
self.addForceAtPos(f·up, hubPos);  B.addForceAtLocalPos(−f·up, basePosition)
```

No bump stops. `packerRange` (+0x2c) and `bumpStopRate` (+0x10) are never loaded for ML, **and
they are not initialised either** (reviewer, disassembly of `SuspensionML::SuspensionML`
0x1402c8e20: the only base-member stores are +0x14 = 0, +0x30 = 1e-7, +0x18 = 0, +0x24 = 0; the
object comes from a plain `operator new(200)` in `Car::Car`). So the packer test in this step reads
whatever was in the heap block until a setup item writes `PACKER_RANGE` / `BUMP_STOP_RATE`. The port
starts both at 0 (no packer). It can only be bit-compared with the game when the game's
uninitialised members are set first: for the whole-car recordings the oracle writes 0 into the four
of them right after the car is built, and the suspension micro-oracle sets both numbers to the same
random values on both sides, so the packer branch is compared too.
`bumpStopUp` / `bumpStopDn` are uninitialised too but never read by ML.

### 5.6 HeaveSpring::step 0x1402b3960 (from pseudo-C; base offsets from disassembly)

Called from `Car::stepComponents` only when `heave.k != 0`. `S0`, `S1` = the left and right
`Suspension` (DWB only), `ref0`, `ref1` their `refPoint`. `Car::initHeaveSprings` 0x140273f40
(confirmed) `dynamic_cast`s all four suspensions to `Suspension` and returns without calling
`HeaveSpring::init` if any of them is not DWB, so one STRUT / AXLE / ML wheel anywhere on the car
disables both heave springs: the ini sections are never read, `k` stays 0 (`HeaveSpring::HeaveSpring`
0x14026daf0), `isPresent` stays false and no heave setup items are registered.

```
l0 = B.worldToLocal(S0.hub.getPosition());  l1 = B.worldToLocal(S1.hub.getPosition())
if S0.k != 0 or S1.k != 0:  rodLength = (S1.rodLength + S0.rodLength)·0.5
avgY   = (l1.y + l0.y)·0.5
travel = (avgY − ref0.y) + rodLength             status.travel = travel
F = (travel·progressiveK + k)·travel
if packerRange != 0 and travel > packerRange:  F += (travel − packerRange)·bumpStopRate
S0.addForceAtPos(−F·up, hub0 pos);  S1.addForceAtPos(−F·up, hub1 pos)
B.addLocalForceAtLocalPos((0, F, 0), ref0);  B.addLocalForceAtLocalPos((0, F, 0), ref1)

d = avgY − ref0.y
if bumpStopUp != 0 and d > bumpStopUp:  Fb = (d − bumpStopUp)·500000   (applied like F)
if bumpStopDn != 0 and d < bumpStopDn:  Fb = (d − bumpStopDn)·500000   (applied like F)

v = ((hubVel0 + hubVel1)·0.5 − (B.getLocalPointVelocity(ref0) + B.getLocalPointVelocity(ref1))·0.5) · up
f = Damper::getForce(v)
S0.addForceAtPos(f·up, hub0 pos);  S1.addForceAtPos(f·up, hub1 pos)
B.addForceAtLocalPos(−f·up, ref0);  B.addForceAtLocalPos(−f·up, ref1)
```

The full force `F` goes to **each** hub (it is not halved). There is no `F > 0` test, so the heave
spring can also pull. If the wheel springs are non-zero, the heave `ROD_LENGTH` from the ini is
replaced every step by the average of the two wheel rod lengths.
In the data this is the normal case (reviewer): of the 7 cars with heave springs only
`tatuusfa1` front has wheel `SPRING_RATE=0`. On every other one, `ks_ferrari_f2004` included
(`[HEAVE_FRONT] ROD_LENGTH=0.00` against wheel −0.010, `[HEAVE_REAR] ROD_LENGTH=0.080` against wheel
−0.040), the heave `ROD_LENGTH` key and the `ROD_LENGTH_HF` / `ROD_LENGTH_HR` setup items are
overwritten before they are used and have no effect. Also note that a wheel with `k == 0` has its
own packer and bump stops switched off by the `k != 0` tests in section 5.2.

### 5.7 AntirollBar::step 0x1402bb640 (from pseudo-C)

```
if ctrl.ready:  k = DynamicController::eval(ctrl)
if k > 0:
    p0 = hubs[0].getHubWorldMatrix().translation;  p1 = same for hubs[1]
    l0 = B.worldToLocal(p0);  l1 = B.worldToLocal(p1)
    delta = l1.y − l0.y
    n = normalize(row 2 of B's matrix)
    hubs[0].addForceAtPos( delta·k·n, p0);  B.addLocalForceAtLocalPos((0, −(delta·k), 0), l0)
    hubs[1].addForceAtPos(−delta·k·n, p1);  B.addLocalForceAtLocalPos((0,  delta·k , 0), l1)
```

`k` is a force per metre of left/right height difference (N/m at the wheel). No motion ratio.
Operation order in the code: each hub force component is `(n_i·delta)·k` (and `(n_i·(−delta))·k`
for hub 1), the body side is `−(delta·k)` and `delta·k`. `p0`, `p1` come from `getHubWorldMatrix`
(slot +0x08), so for an axle they are the wheel-end points of the axle.

### 5.8 Steering, toe and damage: setSteerLengthOffset

`Suspension::setSteerLengthOffset(offset)` 0x1402c3290 (same code in `SuspensionStrut` 0x1402c6540
and `SuspensionML` 0x1402caa40; empty for the axle):

```
side = sign(refPoint.x)                      (+1, −1 or 0)
carSteer.x = damageDirection·damageAmount + offset + side·toeOUT_Linear + baseCarSteerPosition.x
carSteer.y, .z = baseCarSteerPosition.y, .z
core.reseatDistanceJointLocal(steering joint, carSteer (body coords), tyreSteer (hub coords))
```

The steering joint is `joints[4]` (DWB, ML) or `joints[2]` (STRUT).
`PhysicsCore::reseatDistanceJointLocal` 0x1402cd480 (confirmed) converts both local anchors to
world with `dBodyGetRelPointPos`, calls `dJointSetDBallAnchor1/2`, then restores the rod length
with `dJointSetDBallDistance(joint, stored length)`. The stored length is the anchor distance read
back when `PhysicsCore::createDistanceJoint` made the joint. So the rod never changes length: its
body end is moved and the solver pulls the hub end after it over the following steps.
`SteeringSystem::step` 0x1402b81b0 calls it every step for wheels 0 and 1 with
`offset = −(car.finalSteerAngleSignal · linearRatio)`, and for wheels 2 and 3 when `has4ws` is set.
Rear-wheel steering (confirmed, both functions read whole): `SteeringSystem::init` 0x1402b80d0 sets
`has4ws` when the file `ctrl_4ws.ini` exists in the car's data (there is no other switch) and loads
it into the `DynamicController` `ctrl4ws`. The step evaluates the controller once
(`DynamicController::eval` 0x1402b0c00) and passes the same value to wheel 2 and to wheel 3. The
value is the rod shift in metres: it is not negated and not multiplied by `linearRatio`. The step
has no speed term of its own; the two stock files (`ks_ferrari_812_superfast`,
`ks_porsche_panamera`) multiply a `STEER_DEG` table by a `GAS`, a `SLIPANGLE_REAR_MAX` and a
`SPEED_KMH` table, each with its own filter. On a rear axle the slot is the empty function: the
controller is still evaluated and its result is dropped.

Damage: `setDamage(x)` (DWB 0x1402c31e0, STRUT 0x1402c6160, ML 0x1402ca980):
`if x > minVelocity: damageAmount = min((x − minVelocity)·damageGain, maxDamage)`. The function
itself is not cumulative (a smaller `x` above the threshold would lower the amount), but its only
caller cannot pass a smaller value: `Car::onCollisionCallBack` 0x140274650 passes
`x = (damageZoneLevel[a] + damageZoneLevel[b]) · 0.5` (reviewer, disassembly at 0x140274a07 ..
0x140274ab2: wheel 0 uses zones 0 and 2, wheel 1 zones 0 and 3, wheel 2 zones 1 and 2, wheel 3
zones 1 and 3), and each zone level is a running maximum of
`closing speed along the contact normal · 3.6 · ksPhysics->mechanicalDamageRate`, i.e. km/h. So
`MIN_VELOCITY` is in km/h (40 in 90 cars), `damageAmount` is in metres of steering-rod shift, and it
only grows until `resetDamage`. The four `setDamage` calls sit outside the "this contact was hard
enough" block, so they are repeated on every collision callback that involves the car body once
both zones are above 0. `getDamage()` =
`damageAmount / maxDamage` (0 if `maxDamage` is 0). `resetDamage()` zeroes `damageAmount`.
`setDamage` also copies `x` to `lastAmount` when `isDebug` is set (nothing else uses it).
ML never reads `[DAMAGE]`, so its `damageGain` and `maxDamage` stay 0 and its damage is always 0.
The value only takes effect through the next `setSteerLengthOffset` call (a bent toe). Callers of
`setSteerLengthOffset` after construction (confirmed, all through slot +0x28): `SteeringSystem::step`
(wheels 0 and 1 every step; wheels 2 and 3 only with `has4ws`), and two `onValueChanged` lambdas that
`SetupManager::initItems` attaches to the rear toe setup items: 0x140288f00 on `TOE_OUT_LR` calls
`suspensions[2]->setSteerLengthOffset(0)`, 0x140288ee0 on `TOE_OUT_RR` does the same for
`suspensions[3]`. So on a car without four-wheel steering, rear toe and rear damage reach the
geometry only when the rear toe setup item changes (and at construction); between those moments rear
damage is recorded but not applied. On a car with `ctrl_4ws.ini` the rear rods are reseated every
step, so rear toe and rear damage reach the geometry every step, like the fronts. The front toe items have no such lambda because the front rods
are reseated every step anyway.

### 5.9 Static toe, camber, caster in one place

- **Toe** (`TOE_OUT`, `toeOUT_Linear`): a real geometry change. The body end of the steering rod is
  shifted by `sign(refPoint.x)·toeOUT_Linear` metres in body x (section 5.8). The resulting angle
  depends on the steering-arm length; the game never converts it to an angle.
- **Camber** (`STATIC_CAMBER`, `staticCamber`): not in the geometry. It is a fixed rotation added in
  `getHubWorldMatrix` (section 5.10), on top of whatever camber change the links produce.
- **Caster, kingpin inclination, scrub radius, anti-dive/anti-squat, roll centre**: no keys. They
  follow from the pick-up points. The steering axis is whatever `getSteerBasis` returns.
- **Ride height**: `BASEY` places the wheel centre relative to the body origin; `ROD_LENGTH` shifts
  the spring's zero point, so a longer rod means more spring force at the same hub position.

### 5.10 Force entry points used by the tyre (steer torque bookkeeping)

`getSteerBasis(centre, axis)` gives the steering axis: DWB = line through the hub's top and bottom
ball joints (`tyreTopWB`, `tyreBottomWB`, in world), STRUT = line from the strut foot to the top
mount, ML = line through `joints[0].ballTyre` and `joints[2].ballTyre`; `centre` is the midpoint.
Exact form, the same in all three (confirmed): with `a` = the upper point (DWB
`H.localToWorld(tyreTopWB)`, STRUT `B.localToWorld(dataRelToBody.carStrut)`, ML joint 0) and `b` =
the lower one (DWB `H.localToWorld(tyreBottomWB)`, STRUT `H.localToWorld(tyreStrut)`, ML joint 2):
`axis = (a − b) · (1.0/|a − b|)` (length summed y², x², z²; left unnormalised if the length is 0),
`centre = (a + b) · 0.5`. So `axis` points upward. Read in the disassembly for STRUT 0x1402c4d70
and ML 0x1402c97e0 in Task 17: the inverse length multiplies x from the left and y, z from the
right (`inv·d.x`, `d.y·inv`, `d.z·inv`).

The axle has no steering axis. `SuspensionAxle::getSteerBasis` 0x1402c8510 prints
`Kunos Simulazioni: CRITICAL ERROR` and `SuspensionAxle::getSteerBasis not implemented`, then
raises an exception on purpose (`ksGenerateCrash` 0x140239a10: `RaiseException(0x29a, ...)`). It
writes neither output. Nothing calls it for an axle: the other three classes call their own
`getSteerBasis` from their own `addForceAtPos` / `addTorque`. The port panics with the same text.

```
addForceAtPos(F, P, driven, addToSteerTorque):
    H.addForceAtPos(F, P)
    if addToSteerTorque:  steerTorque += ((P − centre) × F) · axis
addTorque(T):
    H.addTorque(T);  steerTorque += axis · T
addLocalForceAndTorque(F, T, driveTorque):
    H.addForceAtLocalPos(F, (0,0,0));  H.addTorque(T)
    steerTorque += ((H.getPosition() − centre) × F) · axis + axis · T      (DWB and STRUT only)
    if driveTorque != 0:  car.body.addTorque(driveTorque)
```

`steerTorque` is reset to 0 at the top of each `step`, so after the tyres have run it holds this
step's total. The axle versions forward to the axle body and keep no steer torque; the `driven`
flag is ignored by all four classes. Axle details (confirmed, 0x1402c7f00 / 0x1402c7fa0 /
0x1402c7f10): `addForceAtPos` and `addTorque` are tail jumps to `axle.addForceAtPos` /
`axle.addTorque`; `addLocalForceAndTorque` calls `axle.addForceAtLocalPos(F, (0,0,0))`, so that
force acts at the **centre of the axle body**, not at the wheel end, then `axle.addTorque(T)` and
the same `driveTorque` rule. ML's `addLocalForceAndTorque` 0x1402c92f0 applies force and torque to
the hub but adds nothing to `steerTorque`. `addTorque` and `addLocalForceAndTorque` are the same code in
DWB and STRUT; the sum order there is `axis.y·T.y + axis.x·T.x + axis.z·T.z + steerTorque`, and in
`addLocalForceAndTorque` the force moment is added (and stored) first, then the torque term. `addForceAtPos` differs in form only: with `r = P − centre`, DWB and ML compute
`steerTorque = ((F.x·r.z − F.z·r.x)·axis.y + (F.z·r.y − F.y·r.z)·axis.x + (F.y·r.x − F.x·r.y)·axis.z) + steerTorque`,
STRUT computes
`steerTorque = steerTorque − ((F.z·r.x − F.x·r.z)·axis.y + (F.y·r.z − F.z·r.y)·axis.x + (F.x·r.y − F.y·r.x)·axis.z)`
(every STRUT term is the negation of the DWB term, so the results agree except possibly in the
sign of a zero; port each as written).

What the tyre passes for the two bools (confirmed in disassembly): `driven` is always
`Tyre::driven` (Tyre +0x5a1). `addToSteerTorque` is 1 at the grip-force call sites
(`Tyre::addTyreForces`, store at 0x14027eb21 before the call at 0x14027eb58;
`Tyre::addTyreForcesV10`, 0x14027f4cf / 0x14027f4d4) and 0 for the vertical load in
`Tyre::addGroundContact` (call at 0x14027db8b). So tyre load does not enter `steerTorque`
directly; grip forces, the tyre torques (`addTorque`) and `addLocalForceAndTorque` do.

`getHubWorldMatrix()` (DWB, STRUT 0x1402c4c50, ML 0x1402c96c0) =
`rotation(axis (0,0,1), staticCamber) × hub world matrix`. So **static camber is not in the
geometry**: the hub body sits upright at design height and the camber angle is added to the matrix
the tyre and the graphics see. The axle version returns the axle matrix shifted by `±track` along
the axle's own x axis, with no camber.

---

## 6. Connections to other systems

### How each type is attached (bodies and joints)

All joints are created through `IPhysicsCore` (`PhysicsCore::createDistanceJoint` 0x1402cc190 slot
+0x28, which takes two world-space anchors, `createSliderJoint` 0x1402cc3f0 +0x68,
`createBallJoint` 0x1402cc010 +0x78). Anchor points are `B.localToWorld(refPoint + point from the
ini)` at creation, with the hub placed at `B.localToWorld(refPoint)` with the body's rotation.

| Type | Bodies | Joints |
|---|---|---|
| DWB (`Suspension::attach`) | hub: `hubMass`, box (0.2, 0.6, 0.6) | 5 distance joints body↔hub: `[0]` carTopWB_R–tyreTopWB, `[1]` carTopWB_F–tyreTopWB, `[2]` carBottomWB_R–tyreBottomWB, `[3]` carBottomWB_F–tyreBottomWB, `[4]` carSteer–tyreSteer (steering rod). ERP 0.3, CFM `baseCFM` set in the constructor. |
| STRUT (`SuspensionStrut::attach`) | hub: `0.8·hubMass`, box (0.2, 0.6, 0.6); strut body: `0.2·hubMass`, box (0.05, 0.5, 0.2), placed 0.1 m down the strut from the top mount | `[0]` distance carBottomWB_R–tyreBottomWB, `[1]` distance carBottomWB_F–tyreBottomWB, `[2]` distance carSteer–tyreSteer (steering rod), `[3]` slider strut body↔hub along the strut axis, `[4]` ball joint body↔strut body at the top mount. No ERP/CFM call in the constructor (confirmed): the joints keep the values ODE gave them at creation until `Car::step` calls `setERPCFM`, and the slider drops the ERP it is given. The hub body is created first, then the strut body, so a strut wheel adds two bodies to the car. `attach` re-poses both bodies; `stop()` stops the hub only, so the strut body keeps its velocity and its accumulated force. |
| AXLE (`SuspensionAxle::SuspensionAxle`, Left instance only) | `Car::rigidAxle`: `[REAR] HUB_MASS`, box (`2·track`, 0.2, 0.5), placed at `axleBasePos` | `LINK_COUNT` distance joints body↔axle, ends `J<i>_CAR` and `J<i>_AXLE`. ERP 0.3, CFM `baseCFM`. The Right instance creates nothing. `Car::Car` creates `Car::rigidAxle` before any hub, so it is the third body of the car: body, fuel tank, axle, hub LF, hub RF (five bodies). The links are created inside the Left constructor, after both front corners, with the car body as body 1 and the axle as body 2. The axle mass has no `≤ 0 → 20` fall-back. |
| ML (`SuspensionML::loadINI`) | hub: `hubMass`, box (0.2, 0.6, 0.6) | 5 distance joints body↔hub, ends `JOINT<i>_CAR` and `JOINT<i>_TYRE`; `[4]` is the steering rod, `[0]` and `[2]` give the steering axis (the hub end of `[0]` must be the upper ball). Hub and joints are made in `loadINI`; `attach` only places the hub. No ERP/CFM call in the constructor and `setERPCFM` is the empty function, so the joints keep the values ODE gave them at creation for the whole session |

`bumpStopJoint` (DWB, STRUT) is the result of `IPhysicsCore` slot +0x30 (`createBumpJoint`), which in
this build is a stub that returns null (`xor eax,eax; ret`). Bump stops are the force code in
section 5, not a joint.

`Car::forcePosition` / `Car::forceRotation` re-run `attach`, which only moves the hub (and strut
body) back to the design position; joints are created once (`joints[0] == null` test).

### Who writes this system's inputs

| Input | Writer | When |
|---|---|---|
| Steering rod offset (`setSteerLengthOffset`, slot +0x28) | `SteeringSystem::step` 0x1402b81b0 | in `stepComponents`, **after** suspensions and tyres of the same step |
| Rear rod reseat with offset 0 (`setSteerLengthOffset(0)`) | `onValueChanged` lambdas of setup items `TOE_OUT_LR` (0x140288f00) and `TOE_OUT_RR` (0x140288ee0) | whenever that setup value changes |
| Tyre load, grip forces, aligning torque (slots +0x18, +0x20, +0xc0) | `Tyre::addGroundContact`, `Tyre::addTyreForces`, `Tyre::addTyreForcesV10`, `Tyre::addTyreForceToHub`, `Tyre::step` | right after `suspension->step` |
| Joint ERP/CFM (`setERPCFM`, slot +0xb8) | `Car::step` 0x140275da0 | start of the step, only for the car with `physicsGUID == 0` |
| `damageData.damageAmount` (`setDamage` +0x78, `resetDamage` +0x80) | `Car::onCollisionCallBack` 0x140274650 (wheel 0 when `damageZoneLevel[0]` and `[2]` are both > 0, wheel 1: `[0]` and `[3]`, wheel 2: `[1]` and `[2]`, wheel 3: `[1]` and `[3]`), `Car::resetSuspensionDamageLevel` 0x140275970 | collision callback / repair |
| `antirollBars[i].k` | `[ARB]`, setup, `DynamicController` (`ctrl_arb_*.ini`), and the dead active-suspension path | |
| `k`, `progressiveK`, `rodLength`, `packerRange`, `bumpStopRate`, `staticCamber`, `toeOUT_Linear`, damper values | `SetupManager` (list below) | when a setup is applied |
| Hub pose and velocity | ODE (`core->step`) | end of the previous step |

### Setup items that point into suspension members (`SetupManager::initItems` 0x140289570, list only)

- `ARB_FRONT`, `ARB_REAR` (×1), `ARB_FRONT_NMM`, `ARB_REAR_NMM` (×1000) → `antirollBars[i].k`
- per wheel `_LF`, `_RF`, `_LR`, `_RR`: `SPRING_RATE` (×1000) → `k`; `PROGRESSIVE_SPRING_RATE`
  (×1000) → `progressiveK`; `ROD_LENGTH` (×0.0001) → `rodLength`; `PACKER_RANGE` (×0.001) →
  `packerRange`; `BUMP_STOP_RATE` (×1000) → `bumpStopRate`; `CAMBER` (×−0.0017453292 for `_LF`,
  `_LR`, ×+0.0017453292 for `_RF`, `_RR`; the item's second factor is 0.1 instead of 1.0) →
  `staticCamber`; `TOE_OUT` (×1e-05) → `toeOUT_Linear`; `DAMP_BUMP`, `DAMP_FAST_BUMP`,
  `DAMP_REBOUND`, `DAMP_FAST_REBOUND` (×1) → the `Damper` returned by `getDamper()` (slot +0x60).
  The items are registered for every suspension class through the base members, so on an AXLE or
  ML wheel they write members the step does not use (e.g. `packerRange` on an axle).
  `TOE_OUT_LR` and `TOE_OUT_RR` additionally get an `onValueChanged` lambda that calls
  `setSteerLengthOffset(0)` on wheel 2 / 3 (section 5.8).
- heave, only if `heaveSprings[i].isPresent`, suffix `_HF` / `_HR`: `SPRING_RATE` (×1000),
  `PROGRESSIVE_SPRING_RATE` (×1000), `ROD_LENGTH` (×0.0001), `PACKER_RANGE` (×0.001),
  `BUMP_STOP_RATE` (×1000), the four `DAMP_*`

### Who reads this system's outputs

| Output | Reader |
|---|---|
| Forces and torques on `Car::body`, the hubs, the strut bodies, `Car::rigidAxle` | ODE integration in `core->step` |
| Hub matrix, point velocity, angular velocity (slots +0x08, +0x10, +0x38) | `Tyre::step` and below; `Car::getTyreMatrix` 0x1402726a0, `Car::getSuspensionMatrix` 0x1402724f0 (plain wrapper of slot +0x08, zero matrix for an index ≥ 4), `Car::getPhysicsState`, `AntirollBar::step` |
| `steerTorque` (`getSteerTorque`, slot +0x30) of wheels 0 and 1 | `Car::getSteerFF` 0x140272180: `mz = lerp toward (st0 + st1)` with `ffFilter`, then gyro term, ×1.4, flat-spot shake, steer assist power curve |
| `status.travel` (`getStatus`, +0x48) | `Car::getPhysicsState` 0x140270d70 (call at 0x1402715c5, copies the first float into `CarPhysicsState::suspensionTravel`), `SharedMemoryWriter::updatePhysics` 0x140186ef0 (calls `getStatus` on the physics car directly), `Telemetry::step` 0x1402bf5b0 (all four wheels, ×1000), `DynamicController::getInput` 0x1402b0d70 (inputs "SusTravelLR/RR" and the rear average, ×1000 = mm), `DynamicWingController::getInput` 0x1402a9ef0 (same, ×1000) |
| `status.damperSpeedMS` | **no reader found** (unverified): every `getStatus` call site checked above reads only `travel`, and no function outside the four `step`s and the constructors names the member (section 8) |
| `getDamage()` (+0x88) | `Car::getPhysicsState`, `Car::getPitstopTime` 0x140271e60 |
| `getMass()` (+0x90) | `Car::calcBodyMass` 0x14026fb70 (body mass = `TOTALMASS` − the four hub masses + ballast, re-applied once per second by `Car::updateBodyMass` 0x140276c70; on a STRUT wheel only the hub's 80 % is subtracted), `Car::getTotalMass` 0x140272570, `Car::stepPreCacheValues` |
| `getBasePosition()` (+0x50) | `Car::Car` (ride pick-up points), `Car::initAeroMap`, `RaceEngineer` (track width, weight distribution, damping ratio, natural frequency, ride height) |
| `getK()` (+0x58), `getDamper()` (+0x60) | `RaceEngineer` damping ratio / natural frequency, `SetupManager` |
| `Car::suspensionTypeR == Axle`, `Car::axleTorqueReaction`, `Car::rigidAxle` | `Drivetrain::step2WD` 0x1402694e0: with `T = locClutch · engine outTorque · current gear ratio`, applies local torque `(0, 0, +T·axleTorqueReaction)` to the body and `(0, 0, −T·axleTorqueReaction)` to the axle (`IRigidBody::addLocalTorque`, slot +0xe0). If `car.torqueModeEx == reactionTorques` it also applies `(−T, 0, 0)` to the axle (to the body instead when `axleTorqueReaction == 0`). |

---

## 7. Port notes

**Effort: M.** The force code is small (four `step` functions of 650 to 1600 bytes, a 119-byte
damper, two helpers) and has no hidden state beyond `status`, `steerTorque` and `damageAmount`. The
bulk is the loader and the joint wiring, and the dependency on rigid bodies and joints behaving
exactly like ODE.

Must exist first:
1. The rigid-body layer (`IRigidBody` with the slots listed in section 5, distance / slider / ball
   joints, `reseatDistanceJointLocal`), see `docs/map/physics_engine.md`.
2. `INIReader` semantics (missing key, `getFloat3`).
3. `mat44f::createFromAxisAngle` and the DirectX matrix multiply for `getHubWorldMatrix`.
4. Nothing from the tyre: the tyre depends on this interface, not the other way round.

Suggested split: (a) `Damper::getForce`, (b) the pure force formulas of the four `step` functions,
`HeaveSpring::step`, `AntirollBar::step`, written against a small rigid-body trait, (c) the loader
with mirroring / rim offset / defaults, (d) joint creation and steering reseat.

Bit-exact test with an oracle (same idea as the tyre oracle):
- **Level 1, no faking:** call `Damper::getForce` 0x1402b3280 directly on a 24-byte struct for a
  grid of speeds and parameter sets. `PIDController::eval` 0x14044e720 likewise.
- **Level 2, recording fake bodies:** build a fake `IRigidBody` whose vtable returns scripted
  values for `getPosition`, `getVelocity`, `getWorldMatrix`, `worldToLocal`, `localToWorld`,
  `getLocalPointVelocity`, `getPointVelocity` and records every `addForceAtPos`,
  `addForceAtLocalPos`, `addLocalForceAtLocalPos`, `addTorque` call (arguments as raw bits, in
  order). Fill a `Suspension` / `SuspensionStrut` / `SuspensionML` / `SuspensionAxle` struct by hand
  (layouts in `re/types/`), point `carBody`, `hub` (and `car->body`, `axle`) at the fakes, call the
  game's `step` and compare the recorded call list plus `status` with Rust. `AntirollBar::step` and
  `HeaveSpring::step` work the same way; they also call the suspension's own vtable (slots +0x08,
  +0x18), so the fake suspension needs the real vtable pointer or a recording one.
  `addForceAtPos(…, true)` and `addTorque` additionally exercise `getSteerBasis`.
- **Level 3, real loader:** needs a real `Car` with `ksPhysics`, a live `PhysicsCore` and the car's
  data path, so it belongs to the "whole car in the oracle" work. A cheaper check for the loader is
  to construct the object in the game process and dump the struct bytes (`dataRelToWheel`,
  `dataRelToBody`, base members, damper) for every car in `cardata/`.
- A first-call guard for a perf counter exists only in three DWB functions (`Suspension::step`,
  `Suspension::setSteerLengthOffset`, `Suspension::addForceAtPos`); it writes globals. The STRUT,
  AXLE, ML, heave and anti-roll-bar code has none. Harmless, but the oracle must run inside the
  game image as the tyre oracle does.
- `SuspensionAxle::attach` / `setPositions`, the AXLE constructor and `SuspensionML::loadINI` call
  `printf` / `mat44f::print` (debug output left in the build); an oracle should expect console
  output from them.
- Three things a port must copy rather than tidy up: the STRUT damper speed has the opposite sign
  to every other type, so its bump and rebound coefficients are swapped (section 5.3); STRUT bump
  stops ignore `BUMP_STOP_RATE` and have no zero test; the heave spring force is applied in full
  to each hub and may pull. One thing it cannot copy: ML reads two uninitialised floats
  (section 5.5); the port starts them at 0. One more thing to copy: on an axle `getSteerBasis`
  raises an exception on purpose (section 5.10).
- Not covered above because they hold no physics: the destructors (only DWB 0x1402c0b70 releases
  its hub body, through `IRigidBody::release` +0x30; STRUT 0x1402c3d00 releases nothing, not even
  the strut body; AXLE 0x1402c7d70 and ML 0x1402c8fc0 only free the joint vector; no destructor
  releases a joint), and `ActiveActuator::ActiveActuator` 0x1402cb5d0 (`targetTravel = 0` +
  default `PIDController`).

---

## 8. Open questions

1. **`useActiveActuator`** is initialised to false and no other function in the named decompile
   refers to it or to `activeActuator` (only the constructor and `Suspension::step`). Treated as a
   dead feature. Not checked by raw offset search for writes to `+0x60`.
2. **ERP/CFM gate.** `Car::step` switches suspension ERP/CFM only when `Car::physicsGUID == 0`. Other
   cars keep whatever the constructors set (DWB and AXLE 0.3 / 1e-7; the STRUT and ML constructors
   make no call, confirmed in the disassembly in Task 17, so their joints keep the values ODE gave
   them at creation). On the first car a STRUT forwards `setERPCFM` to all five joints, but its
   slider drops the ERP (`dJointSetSliderParam`; the CFM goes to a member that an unpowered slider
   without stops never reads). An ML wheel never changes its joints at all. `physicsGUID` is set in `Car::Car`
   to the number of cars already in `ksPhysics->cars`, so 0 is the first car created (confirmed);
   that this is always the player's car is an assumption. `SuspensionML::setERPCFM` is the shared
   empty function. The joint `setERPCFM` (0x1402cd5a0) only writes a value that is > 0.
3. **`dataRelToBody` holds `localToWorld` results** but is later used as body-local coordinates
   (steering reseat, strut step). This is only consistent if the body is at the origin with no
   rotation when `Car::Car` runs. Resolved by the reviewer: `Car::body` comes straight from
   `PhysicsCore::createRigidBody` 0x1402cc3b0 (`dBodyCreate`, so origin and identity rotation), and
   between that and the suspension loop `Car::Car` only calls `Car::initCarData` 0x140272b30 (sets
   mass / inertia on the body, positions only the fuel tank body) and `BrakeSystem::init`
   (does not touch the body). No `setPosition` / `setRotation` reaches the body before the
   suspensions are built, so `localToWorld` is the identity there and `dataRelToBody` really is
   body-local. (ML does it explicitly: `relToCar = B.worldToLocal(H.localToWorld(point))`.)
4. **Frame of `getLocalPointVelocity`** (IRigidBody +0x100): resolved by the reviewer.
   `RigidBodyODE::getLocalPointVelocity` 0x1402ce4e0 is `dBodyGetRelPointVel` (world velocity of a
   body-local point); `getPointVelocity` 0x1402ce600 is `dBodyGetPointVel` (world point).
   `addLocalForceAtLocalPos` = `dBodyAddRelForceAtRelPos`, `addForceAtLocalPos` =
   `dBodyAddForceAtRelPos`, `addForceAtPos` = `dBodyAddForceAtPos`.
5. **`INIReader::getFloat` on a missing key.** `INIReader::getFloat` 0x1402358c0 returns 0.0 when
   the string from `getString` is empty (confirmed). That `getString` returns an empty string for a
   missing key or section was not opened (assumed).
6. **Matrix conventions.** The order in `getHubWorldMatrix` is confirmed in the disassembly of
   0x1402c4c50: the camber rotation from `mat44f::createFromAxisAngle` is passed as M1 (rows in
   xmm0..xmm3), the hub matrix as M2 (pointer), and the four result rows come back in xmm0..xmm3, so
   the result is `XMMatrixMultiply(rotation, hub)`. (The pseudo-C only tracks xmm0, which is why it
   looks garbled.) "Row 2 = body up" is the layout reading (`M21, M22, M23` at +0x10) and is
   consistent with the body-local `(0, F, 0)` forces, but the axis convention of
   `RigidBodyODE::getWorldMatrix` was not opened here.
7. **Argument of `setDamage`.** Resolved by the reviewer: it is the mean of the two
   `Car::damageZoneLevel` entries that gate the call (section 5.8). What stays open is who resets
   `damageZoneLevel` and which collider categories (the `== 1` / `== 0x10` tests at the top of
   `Car::onCollisionCallBack`) are excluded from raising it; that belongs to the collision / damage
   map.
8. **Fourth argument of `addForceAtPos` from the tyre force calls.** Resolved by the reviewer in
   the disassembly: both grip-force call sites store 1 in the stack slot, `Tyre::addGroundContact`
   stores 0, and the third argument is `Tyre::driven` everywhere (section 5.10).
9. **Rear damage / rear toe.** The first version of this document said nothing calls
   `setSteerLengthOffset` on the rear wheels after construction. That was wrong: the `TOE_OUT_LR` /
   `TOE_OUT_RR` setup items carry lambdas (0x140288f00, 0x140288ee0) that call it with offset 0
   (section 5.8). When `SetupItem` fires `onValueChanged` (on every set, or only on a changed
   value; on setup load, in the pits, both) was not traced, so how often rear damage actually
   reaches the geometry is still open. Callers were searched by direct call and by the pattern
   "slot +0x28 on an element of `Car::suspensions`"; a call through a cached pointer could be missed.
10. **Strut bump stops without zero tests**: confirmed by the reviewer in the disassembly of
    `SuspensionStrut::step` (a single `comiss` against `[this+0x1c]` and against `[this+0x20]`, no
    test against zero, constant 500000.0). With `BUMPSTOP_UP = 0` or `BUMPSTOP_DN = 0` a strut
    therefore has a bump stop sitting exactly at the design position. **Heave force not halved /
    allowed to pull**: confirmed in the disassembly of `HeaveSpring::step` (second reviewer): no
    compare against zero between the packer block and the calls, the same `up·(−F)` vector goes to
    both hubs through slot +0x18 with both bools 0, and `(0, F, 0)` goes to the body twice through
    slot +0xf0, at `ref0` and at `ref1`.
11. **`SuspensionStrut::loadINI` and `SuspensionML::loadINI`** were read in condensed form; the
    key → member mapping of the strut was cross-checked against the store offsets in the
    disassembly. The reviewer confirmed the strut rim-offset block: seven stores, to the x of
    `carStrut`, `carBottomWB_F`, `carBottomWB_R`, `tyreStrut`, `tyreBottomWB`, `tyreSteer`,
    `carSteer`. The reviewer also found that `SuspensionML::loadINI` has none of the damper
    fall-backs (section 4) and that the ML constructor never calls `setSteerLengthOffset`, so ML
    static toe is not applied until the first steering / setup call.
12. **`Suspension::steerAngle`** (+0x1ec) and `steerLinkBaseLength` are written at init; no reader
    was found. `getDebugLines` was not analysed (debug drawing).
13. **Caster / KPI** are not parameters: they come from the pick-up points. `RaceEngineer::getCasterRAD`
    0x14027bb20 computes a value for display; not analysed.
14. **`NetCarStateProvider::initPhysicsValues`** 0x140119fe0 and `CarLodManager::loadLod`
    0x1400e4370 also open `suspensions.ini` (remote cars / graphics). From their string lists:
    the first uses `[BASIC] WHEELBASE`, `CG_LOCATION` and `[FRONT]/[REAR] BASEY`, `TRACK` (wheel
    positions of remote cars); the second shows no suspension key names next to the file name.
    Their code was not read.
15. **`ML` has no stock car.** Task 17 read the whole class in the disassembly and checked it with
    a made-up car, `gt3_multilink` (the Ferrari 488 GT3 with every corner turned into `TYPE=ML`):
    the game's own objects and the port agree bit for bit (`docs/port/suspensions.md` sections 3.1
    and 3.3, and section 7 point 4).
16. **`status.damperSpeedMS` has no reader that could be found.** It is written by all four
    `step`s; `Car::getPhysicsState`, `SharedMemoryWriter::updatePhysics`, `Telemetry::step` and the
    two dynamic controllers read only `travel` from `getStatus()`. The search was by member name and
    by "slot +0x48 on an element of `Car::suspensions`"; a reader through a cached
    `SuspensionStatus*` cannot be excluded. `HeaveSpring` stores no damper speed at all.
17. **Evaluation order of sums** in section 5 (dot products, squared lengths). Read in the
    disassembly: `Suspension::step`, `SuspensionStrut::step`, `SuspensionAxle::step`,
    `SuspensionML::step` (whole), `HeaveSpring::step` (up to the first bump stop)
    and `Suspension::setSteerLengthOffset`; all match the text. Task 17 read the rest for STRUT and
    ML in the disassembly: `getSteerBasis`, the `addForceAtPos` / `addTorque` /
    `addLocalForceAndTorque` steer-torque sums and `setSteerLengthOffset`, and also
    `AntirollBar::step`. All match the text, and the briefs found the STRUT and ML code equal to
    the DWB's except where section 5.10 says otherwise. The heave damper-speed sum was not re-read
    in Task 17.
18. **Uninitialised members.** `SuspensionML` reads `packerRange` and `bumpStopRate` that nothing
    initialises (section 5.5); `SuspensionAxle` leaves `staticCamber` and `packerRange`
    uninitialised but does not read them (section 4). Whether anything outside these classes reads
    the base floats directly (other than the setup items, which hold pointers to them) was not
    searched.
19. **STRUT damper sign** (section 5.3): confirmed as code, intent unknown. The port copies it as
    written and matches the game bit for bit on three strut cars (`bmw_m3_e30`, `ks_toyota_gt86`,
    `ks_audi_sport_quattro`; `docs/port/suspensions.md` sections 3.1 and 3.3), so it must stay as
    it is.

---

## 9. The ISuspension vtable

Vtables: `Suspension` 0x1404ff870, `SuspensionStrut` 0x1404ffc80, `SuspensionAxle` 0x1404ffe90,
`SuspensionML` 0x1405001a0. Shared stubs: 0x140017870 = empty function; 0x14044f230 = `return 0.0f`;
0x14029cf60 = `return *(float*)(this + 0x2c)`; 0x1402c1be0 = `return *(float*)(this + 8)`.
Signatures are from the PDB symbols; parameter names are mine.

| Slot | Method | What it does | DWB | STRUT | AXLE | ML |
|---|---|---|---|---|---|---|
| +0x00 | destructor | | 0x1402c0b70 | 0x1402c3d00 | 0x1402c7d70 | 0x1402c8fc0 |
| +0x08 | `mat44f getHubWorldMatrix()` | hub world matrix with static camber applied (axle: axle matrix moved to the wheel, no camber) | 0x1402c4c50 | 0x1402c4c50 | 0x1402c8440 | 0x1402c96c0 |
| +0x10 | `vec3f getPointVelocity(const vec3f& worldPos)` | velocity of the hub body at a world point | 0x1402c4d40 | 0x1402c4d40 | 0x1402c84e0 | 0x1402c97b0 |
| +0x18 | `void addForceAtPos(const vec3f& force, const vec3f& worldPos, bool driven, bool addToSteerTorque)` | world force at a world point on the hub; optionally adds its moment about the steering axis to `steerTorque` | 0x1402c0bf0 | 0x1402c3d60 | 0x1402c7f00 | 0x1402c91d0 |
| +0x20 | `void addTorque(const vec3f& torque)` | world torque on the hub; its component along the steering axis is added to `steerTorque` (not for the axle) | 0x1402c0f10 | 0x1402c4060 | 0x1402c7fa0 | 0x1402c9380 |
| +0x28 | `void setSteerLengthOffset(float offset)` | moves the body end of the steering rod sideways (steer + toe + damage) and reseats the joint | 0x1402c3290 | 0x1402c6540 | empty | 0x1402caa40 |
| +0x30 | `float getSteerTorque()` | `steerTorque` accumulated this step | 0x1402c1d60 | 0x1402c4ed0 | returns 0 | 0x1402c9940 |
| +0x38 | `vec3f getHubAngularVelocity()` | angular velocity of the hub body | 0x1402c1bc0 | 0x1402c1bc0 | 0x1402c8420 | 0x1402c96a0 |
| +0x40 | `void attach()` | puts the hub back at its design pose; first call creates the joints (DWB, STRUT) | 0x1402c0fb0 | 0x1402c4100 | 0x1402c7fb0 | 0x1402c9420 |
| +0x48 | `SuspensionStatus& getStatus()` | reference to `status` (travel, damperSpeedMS) | 0x1402c1bf0 | 0x1402c4d60 | 0x1402c8500 | 0x1402c97d0 |
| +0x50 | `vec3f getBasePosition()` | design wheel-centre position in body coordinates | 0x1402c4690 | 0x1402c4690 | 0x1402c7fc0 | 0x1402c9430 |
| +0x58 | `float getK()` | `k` | 0x1402c1be0 | 0x1402c1be0 | 0x1402c1be0 | 0x1402c1be0 |
| +0x60 | `Damper* getDamper()` | pointer to the damper (used by setup and `RaceEngineer`) | 0x1402c1520 | 0x1402c46d0 | 0x1402c8000 | 0x1402c9470 |
| +0x68 | `float getPackerRange()` | `packerRange` (axle: 0) | 0x14029cf60 | 0x14029cf60 | returns 0 | 0x14029cf60 |
| +0x70 | `vector<DebugLine> getDebugLines(const mat44f&, const mat44f&)` | lines for the debug renderer | 0x1402c1530 | 0x1402c46e0 | 0x1402c8010 | 0x1402c9480 |
| +0x78 | `void setDamage(float amount)` | sets `damageAmount` from an impact figure | 0x1402c31e0 | 0x1402c6160 | empty | 0x1402ca980 |
| +0x80 | `void resetDamage()` | `damageAmount = 0` | 0x1402c31d0 | 0x1402c6150 | empty | 0x1402ca970 |
| +0x88 | `float getDamage()` | `damageAmount / maxDamage` | 0x1402c1500 | 0x1402c46b0 | returns 0 | 0x1402c9450 |
| +0x90 | `float getMass()` | hub body mass (axle: half the axle mass; strut: the hub only, 80 % of `HUB_MASS`) | 0x1402c4d30 | 0x1402c4d30 | 0x1402c84c0 | 0x1402c97a0 |
| +0x98 | `void stop()` | `hub->stop(1.0)` | 0x1402c6b20 | 0x1402c6b20 | 0x1402c8db0 | 0x1402cad90 |
| +0xa0 | `vec3f getVelocity()` | hub linear velocity (axle: velocity of the axle at `(±track, 0, 0)`) | 0x1402c1d70 | 0x1402c1d70 | 0x1402c85a0 | 0x1402c9950 |
| +0xa8 | `void getSteerBasis(vec3f& centre, vec3f& axis)` | steering axis in world space (axle: prints "not implemented" and crashes on purpose) | 0x1402c1c00 | 0x1402c4d70 | 0x1402c8510 | 0x1402c97e0 |
| +0xb0 | `void step(float dt)` | spring, damper, bump stops (section 5) | 0x1402c3390 | 0x1402c6600 | 0x1402c8770 | 0x1402cab00 |
| +0xb8 | `void setERPCFM(float erp, float cfm)` | forwards to every joint | 0x1402c3230 | 0x1402c61b0 | 0x1402c8680 | empty |
| +0xc0 | `void addLocalForceAndTorque(const vec3f& force, const vec3f& torque, const vec3f& driveTorque)` | world force at the hub centre + world torque on the hub, steer-torque bookkeeping (DWB, STRUT), and `driveTorque` (if non-zero) as a torque on the car body | 0x1402c0d40 | 0x1402c3e80 | 0x1402c7f10 | 0x1402c92f0 |

Names for the slots that `docs/map/tyre.md` section 7 item 1 described by behaviour:

| Slot | tyre.md description | Real name |
|---|---|---|
| +0x08 | wheel world matrix | `getHubWorldMatrix` (includes static camber) |
| +0x10 | point velocity | `getPointVelocity` |
| +0x18 | force at point | `addForceAtPos(force, pos, driven, addToSteerTorque)` |
| +0x20 | torque | `addTorque` |
| +0x38 | hub angular velocity | `getHubAngularVelocity` |
| +0xc0 | local force + torque | `addLocalForceAndTorque(force, torque, driveTorque)`; despite the name the force is applied with `IRigidBody::addForceAtLocalPos` at the hub origin |
