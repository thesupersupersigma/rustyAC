# Car body: mass, fuel, damage, collisions

Source: `acs.exe` + `acs.pdb`, read only through the local material (`re/decomp/`, `re/types/`,
`tools/re_query.py`, `tools/disasm.py`, `tools/ini_survey.py`), the 113 extracted cars in
`cardata/` and Kunos' commented example car (`sdk/dev/content/cars/formula_k/data/`, read only).
Raw pseudo-C of every function cited here: `re/car/body/` (plus two disassembly listings).
Nothing in the game folder, the Ghidra project, a running game or git was changed. No Rust was
written. Statements marked *(disassembly)* were checked instruction by instruction; everything
else was read from the pseudo-C; interpretation is flagged as such.

---

## 1. Plain-English summary

A car in the game is a heavy box (the body) with four small heavy lumps (the wheel hubs) hung on
it by rods, and a second small box (the fuel tank) bolted to it.
When the car is built, the game reads one total weight from the car's data file, takes away the
weight of the four hubs, and what is left is the weight of the body; how hard the body is to
rotate is worked out as if it were a plain box of a given width, height and length.
The fuel is a separate weight sitting where the data file says the tank is, so a full tank makes
the car heavier and shifts its balance, and an emptying tank does the opposite.
Every step the engine burns a little fuel, in proportion to its revs, how much throttle is
used and the turbo pressure; once a second the weight of the tank (and of the body, in case
ballast changed) is refreshed.
The body is surrounded by two kinds of invisible shapes: one or two flat slabs under the floor
that only touch the road and kerbs, and a rough 3D shell of the whole car that touches walls,
other cars and loose objects (and the road too, but only once the car is lying on its side).
While a car is in the pit lane its shell stops touching other cars, and it stays that way until
it has left the pit lane and no other car is within six metres.
Whenever the shell hits something, the game measures how fast the two were closing at the
contact point; that speed, in km/h, becomes the damage of the front, rear, left or right of the
car (the worst hit so far is kept), bends the suspension of the matching corner, hurts the
wings, and above 150 km/h kills the engine.
Hits against the road surface or against loose objects never cause damage.
A car that stands still on all four wheels with no drive for 50 steps is frozen in place so it
does not creep; any movement wakes it.
Moving a car by hand (to the pits, to the grid) puts the body, tank, hubs and wheels back in
their rest pose, refills the fuel, clears speeds and (through the drivetrain reset) also gives
the engine its full life back; body and suspension repairs are a separate call that sets the
damage numbers back to zero.

---

## 2. Classes

| Class | Size | What it is for |
|---|---|---|
| `Car` | 0x3ea0 | The whole car. Owns every component by value (tyres, drivetrain, aero, …), the ODE bodies `body`, `fuelTankBody`, optional `rigidAxle`, the `fuelTankJoint`, the fuel state, the five damage numbers, the collision bookkeeping. Layout: `re/types/Car.txt`. |
| `CarColliderManager` | 0x80 | Reads `colliders.ini` and creates the floor boxes on `Car::body`. `step` only hot-reloads the file while `isLive` is set; the only writer found is a console-variable handler in `CarAvatar` (lambda at `0x1400cf380`). The reload first calls `body->removeCollisionObjects` (+0x20), which is the shared empty function, so the reloaded boxes are added on top of the old ones (developer tool only). |
| `CarCollisionBox` | 0x20 | One floor box: `centre`, `size`, `id` (the ODE geom handle). |
| `CarCollisionBounds` | 0x28 | Bounding box of the collider mesh (`min`, `max`, `length`, `width`, `lengthFront`, `lengthRear`). Not read by the physics step; no reader was found by member name. |
| `ThermalObject` | 0x20 | A one-number heat model. The only instance is `Car::water` (coolant temperature, display only). |
| `PitStopTimings` | 0x14 | Five pit-stop durations from `car.ini [PIT_STOP]`. Rules, not physics. |
| `OnCollisionEvent` | 0x28 | Payload of `Car::evOnCollisionEvent`: other body, closing speed, world position, position in the car frame, collider group. |
| `ACPhysicsEvent` | 0x48 | Record pushed to `PhysicsEngine::eventQueue` for the main thread (sound, sparks). |
| `PhysicsObject` | 0x20 | A loose track object (cone, tyre stack): one ODE body with a mesh collider. Created by `TrackObject::TrackObject` @ `0x1401cf1a0`. |
| `RigidBodyODE` (`IRigidBody`) | 0x48 | Wrapper of one ODE body; mass and collider methods are described here, the rest in `docs/map/physics_engine.md`. |
| `RBCollisionMesh` / `CollisionMeshODE` (`ICollisionObject`) | 0x20 / 0x30 | User data of a mesh geom on a body / of a static track mesh. `getGroup()` (vtable +0x18) returns the category the mesh was created with. Box geoms have **no** user data. |
| `SurfaceDef` | 0xc8 | Track surface description. Only `collisionCategory` and `isPitlane` matter here. |
| `SusDamageDef` | 0x1c | Per-suspension damage state (`minVelocity`, `damageGain`, `maxDamage`, `damageAmount`). Owned by the suspension; written through `ISuspension::setDamage`. |

---

## 3. Key functions

### 3.1 Loading / init

```
CarAvatar::initPhysics                         0x1400d7660   graphics side, one per car
├─ Car::Car(PhysicsEngine*, unixName, configName)            0x14026bf00   (object is operator_new(0x3ea0))
│  ├─ member constructors (CarColliderManager 0x14026d7b0, Drivetrain, Tyre x4, BrakeSystem, ...)
│  ├─ physicsGUID = ksPhysics->cars.size()
│  ├─ Car::initCarDataPath                     0x1402739c0   carDataPath = "content/cars/<unixName>/data/"
│  ├─ fuel = maxFuel = requestedFuel = 30
│  ├─ body         = core->createRigidBody()   (IPhysicsCore +0x18 -> PhysicsCore::createRigidBody 0x1402cc3b0)
│  ├─ fuelTankBody = core->createRigidBody()
│  ├─ Car::initCarData                         0x140272b30   car.ini
│  │  ├─ Car::getConfigPath                    0x140270420   data_<configName>/ override
│  │  ├─ body->setMassExplicitInertia (+0x00)  or  body->setMassBox (+0x38)   with mass = TOTALMASS
│  │  ├─ fuelTankBody->setMassBox(1, 0.5, 0.5, 0.5); fuelTankBody->setPosition(fuelTankPos)
│  │  ├─ fuelTankJoint = core->createFixedJoint(fuelTankBody, body)   (+0x80 -> 0x1402cc2a0)
│  │  ├─ water.tmass = 20, water.coolSpeedK = 0.002
│  │  └─ Car::initPitstopTimings               0x140274020   car.ini [PIT_STOP]
│  ├─ BrakeSystem::init
│  ├─ suspensions.ini: [REAR] TYPE == AXLE -> rigidAxle = core->createRigidBody(), [AXLE] TORQUE_REACTION
│  ├─ for wheel 0..3: new Suspension / SuspensionStrut / SuspensionML / SuspensionAxle  (creates hub bodies, joints)
│  │                  Tyre::init(tyres[i], suspension, ksPhysics->track, ...)          0x140280650
│  ├─ Car::initHeaveSprings, ridePickupPoint[0/1].z = suspension 0 / 2 base position z
│  ├─ Car::initAeroMap, ERS::init (Kers::init if no ERS), SteeringSystem::init, Drivetrain::init,
│  │  Autoclutch / AutoBlip / AutoShifter / GearChanger / EDL init, Car::buildARBS, ABS / TractionControl /
│  │  SpeedLimiter init
│  ├─ ksPhysics->cars.push_back(this)
│  ├─ Car::updateBodyMass                      0x140276c70   first real body mass (hubs now exist)
│  │  └─ Car::calcBodyMass                     0x14026fb70
│  ├─ CarColliderManager::init                 0x1402a3750
│  │  └─ CarColliderManager::loadINI           0x1402a37a0   colliders.ini -> body->addBoxCollider (+0xc8)
│  ├─ SetupManager::init, Telemetry::init (car 0 only), SplineLocator::init, StabilityControl::init,
│  │  DriftModeComponent::init, PerformanceMeter::init, LapInvalidator::init, PenaltyManager::init
│  ├─ damageZoneLevel[0..4] = 0
│  ├─ TimeTransponder::init, SlipStream::init, FuelLapEvaluator::init
│  ├─ ksPhysics->evOnNewSessionPhysics += Car::onNewSession (0x140274c30)
│  ├─ ksPhysics->evOnStepCompleted     += Car::postStep     (0x140275430)
│  ├─ fuel_cons.ini [FUEL_EVAL] KM_PER_LITER -> expectedFuelPerLap
│  ├─ RaceEngineer::evaluateFuelPerLapFromTrackSpline        0x14027ac60   (fallback / log line)
│  └─ carHalfWidth, powerClassIndex = maxPowerW / Car::getTotalMass(true)   0x140272570
├─ (fills CarAvatar::physicsInfo from the new Car)
└─ if content/cars/<name>/collider.kn5 exists:
   ├─ KN5IO::load                              0x1402151a0   (needs the GraphicsManager)
   ├─ CarAvatar::makeBodyMatrix                0x1400d8ec0   identity shifted by GRAPHICS_OFFSET (+ pitch)
   └─ Car::initColliderMesh(mesh, matrix)      0x140273b20
      └─ body->addMeshCollider (+0x120 -> RigidBodyODE::addMeshCollider 0x1402ce080)
```

Run-time setters reached from outside the step: `Car::setBallastKG` @ `0x1402759e0`,
`Car::setRestrictor` @ `0x140275d10`, `Car::setDamageLevel` @ `0x140275b20` (all zones) and its
per-zone overload at `0x140275b50`, `Car::resetSuspensionDamageLevel` @ `0x140275970`,
`Car::setRequestedFuel` @ `0x140275cb0`, `Car::setFuelForLaps` @ `0x140275b60`,
`Car::forcePosition` @ `0x14026fe10`, `Car::forceRotation` @ `0x140270040`.
Who calls them (from the caller lists in `re/decomp`): most go through lambdas that `CarAvatar`
queues for the physics thread (ballast, restrictor, damage level, teleport), but not all:
`ACClient::onMessageTCP` @ `0x14004c840` also calls `setBallastKG` / `setRestrictor` directly;
`setRequestedFuel` is called from two `PitStop` lambdas, a `SetupFuelManager` lambda and
`AIDriver::generatePitStopStrategy`; `setFuelForLaps` only from `AIDriver`; the per-zone
`setDamageLevel` only from a `Sim` lambda; `forcePosition` also from `AIDriver`,
`PenaltyManager::addJumpStartPenalty` and `Car::step` itself (which thread the direct
`ACClient` calls run on was not traced).

### 3.2 Every physics step

```
PhysicsEngine::step(dt = 0.003, physicsTime, gameTime)       0x140264760
├─ Car::stepPreCacheValues (each car)          0x1402768c0   valueCache.speed = |body velocity|; powerClassIndex =
│                                                            maxPowerW / (body + 4 hub masses), WITHOUT fuel (the
│                                                            constructor's value, with fuel, lives until the first step)
├─ Car::step (each car)                        0x140275da0
│  ├─ car 0 only: suspension->setERPCFM (+0xb8) x4, fuelTankJoint->setERPCFM (+0x08)
│  ├─ Car::pollControls                        0x140274e70
│  ├─ controlsProvider->(+0x10)(4)  lights toggle          (no null test before the call)
│  ├─ black flag and not in pits: Car::forceRotation, Car::forcePosition
│  ├─ Car::updateAirPressure                   0x140276ae0   (aero map)
│  ├─ fuel consumption, acEngine.fuelPressure
│  ├─ Car::updateBodyMass                      0x140276c70   acts once per 1000 ms of physicsTime
│  ├─ control overrides (locked controls, gentle stop, penalties), finalSteerAngleSignal
│  ├─ Autoclutch::step
│  ├─ sleeping rule: body->stop (+0x70), fuelTankBody->stop
│  ├─ accG, lastVelocity
│  ├─ Car::stepThermalObjects                  0x1402769f0   -> ThermalObject::step 0x1402b2f00
│  ├─ Car::stepComponents                      0x1402764d0   ... CarColliderManager::step 0x1402a3d50 ...
│  ├─ Car::updateColliderStatus                0x140276df0   body->setMeshCollideMask (+0x130)
│  └─ car 0 only: Car::stepJumpStart           0x140276780
├─ core->step(dt)                              PhysicsCore::step 0x1402cd690
│  └─ PhysicsCore::collisionStep 0x1402cbf90 -> nearCallback 0x1402ccc70 -> PhysicsCore::onCollision 0x1402ccda0
│     └─ collisionCallback->(+0x08): PhysicsEngine::onCollisionCallBack       0x140264020
│        └─ Car::onCollisionCallBack (called for EVERY car, each filters)     0x140274650
│           ├─ Engine::blowUp                  0x140285a30
│           ├─ suspension->setDamage (+0x78)   e.g. Suspension::setDamage 0x1402c31e0
│           ├─ ksPhysics->eventQueue.push(ACPhysicsEvent)   -> consumed by Sim::stepPhysicsEvent 0x14019ebd0
│           └─ evOnCollisionEvent handlers
└─ evOnStepCompleted handlers: Car::postStep   0x140275430   (fires the car's own evOnStepComplete handlers,
                                                             then slipstream position, spline data)
```

### 3.3 Other functions in these source files

Small functions of `Car.obj`, `CarColliderManager.obj`, `PhysicsObject.obj`, `ThermalObject.obj`
that are not described elsewhere in this document (all read from the pseudo-C):

| Function | What it does |
|---|---|
| `Car::getVelocity` / `getLocalVelocity` / `getLocalAngularVelocity` | forward to `body` (+0x78 / +0xb8 / +0xb0) |
| `Car::getSpeed` | returns `valueCache.speed` (m/s, set by `stepPreCacheValues`); about 45 callers |
| `Car::isInPitLane` | any tyre's `surfaceDef->isPitlane` (same test as `updateColliderStatus`) |
| `Car::isInPits` | body origin within 3 m (`dist² ≤ 9`) of `pitPosition` |
| `Car::getGroundWindVector` | wind vector projected on the plane through three tyre contact points, × 0.44 (aero map) |
| `Car::getBackDistanceFromCar` | spline distance to another car: `(npos − other.npos, wrapped) × spline length` |
| `Car::getTotalKM` | `fuelLapEvaluator.totalM × 0.001` |
| `Car::getFuelPerLap` | `FuelLapEvaluator::getFuelPerLap()`, or `expectedFuelPerLap` while that is ≤ 0 |
| `Car::lockControls` / `lockControlsUntil` | set `isControlsLocked` / `lockControlsTime` (read at the top of `Car::step`) |
| `Car::setBlackFlag`, `addPenalty`, `clearPenalty`, `getPenaltyTime`, `isMinSpeedPenaltyClearDisabled` | rules; not body physics |
| `Car::onNewSession` @ `0x140274c30` | clears `isRetired` and the black flag, resets the performance meter. It does **not** touch fuel, damage or `lastBodyMassUpdateTime` |
| `Car::onTyresStepCompleted` (`tyres[3].onStepCompleted`) | force feedback: `lastFF`, then `controlsProvider->(+0x18)`; steering map |
| `Car::getSteerFF`, `getFinalFF`, `getFrontWheelAngle`, `getWheelSterAngleDEG`, `getEngagement`, `getAIState`, `getWingState`, `getTyreMatrix`, `getSuspensionMatrix`, `getTyreThermalState`, `setSlipStreamEffects`, `resetSplineLocator` | getters / setters of other systems (steering, AI, aero, tyres); not mapped here |
| `Car::initAeroMap`, `initHeaveSprings`, `buildARBS` | aero and suspension maps |
| `CarColliderManager::getBox` | returns one `CarCollisionBox` (debug renderer) |
| `PhysicsObject::getWorldMatrix` / `setWorldMatrix` | forward to the loose object's body |
| `ThermalObject::addHeadSource` | `heatAccumulator += value` |

---

## 4. Data it reads

Paths are relative to the game folder. The main ini files and most tables go through `Car::getConfigPath` (not covered by that override, because they are opened with the plain `data/` path: `fuel_cons.ini`, the TC / ABS level tables, every `DynamicController` file except `ctrl_arb_*.ini` and `ctrl_ers_<n>.ini`, the kers.ini curve and controller files, and the aero controller tables)
@ `0x140270420`: if the car was created with a non-empty `configName` and the plain file
`content/cars/<unixName>/data_<configName>/<file>` exists, that file is used instead.
`INIReader::INIReader` @ `0x1402340a0` then checks whether `<folder of the file>.acd` exists
(i.e. `content/cars/<name>/data.acd`); if yes the text comes out of the encrypted archive
(`INIReader::loadEncrypt` @ `0x140237570` → `FolderEncrypter::decryptFile` @ `0x14023bdd0`), if
not the plain file is read (`INIReader::load` @ `0x140237140`). So **`data.acd` wins over the
`data/` folder**. `INIReader::getFloat` falls through to `return 0.0` when it does not find the
key (that is what the `== 0` default tests below rely on).

### 4.1 `car.ini` (113 of 113 cars) — `Car::initCarData` @ `0x140272b30`

| Key | Member | Default / rule | Unit | Notes |
|---|---|---|---|---|
| `[INFO] SCREEN_NAME` | `screenName` | — | text | display only |
| `[BASIC] TOTALMASS` | `mass` | none (0 if missing) | kg | Kunos comment: "total vehicle weight with driver and no fuel". There is no separate driver mass anywhere in the code |
| `[BASIC] INERTIA` = x,y,z | `bodyInertia` | read only when `[EXPLICIT_INERTIA]` is absent | m | **Side lengths of a solid box** (width, height, length), not moments of inertia. All 113 cars |
| `[EXPLICIT_INERTIA] INERTIA` = x,y,z | `explicitInertia` | section optional; **0 of 113 cars** | kg·m² | Switches the body to `setMassExplicitInertia`; see §5.1 for the bug in that path |
| `[FUEL_EXT] KG_PER_LITER` | `fuelKG` | 0.74 (constructor) | kg/l | optional; 2 of 113 cars (0.76) |
| `[CONTROLS] FFMULT` | `ffMult` = value × 0.001 | always overwritten (0 if missing; the constructor's 0.003 does not survive) | — | steering/FFB map |
| `[CONTROLS] STEER_LOCK` | `steerLock` | always overwritten (constructor 200 does not survive) | deg | steering map |
| `[CONTROLS] STEER_RATIO` | `steerRatio` | always overwritten (constructor 12 does not survive) | — | steering map |
| `[CONTROLS] LINEAR_STEER_ROD_RATIO` | `steerLinearRatio` | 0 → 0.003 | — | steering map |
| `[CONTROLS] STEER_ASSIST` | `steerAssist` | 0 → 1.0 | — | steering map |
| `[FUEL] CONSUMPTION` | `fuelConsumptionK` (double) | 0 | see §5.2 | |
| `[FUEL] FUEL` | `fuel` (double), `requestedFuel` | 0 → 30 | litres | start fuel |
| `[FUEL] MAX_FUEL` | `maxFuel` (double) | 0 → 30 | litres | only read by `setFuelForLaps`, `AIDriver::generatePitStopStrategy`, `RaceEngineer::evaluateFuelPerLapFromTrackSpline` (the fuel-per-lap estimate, which also reads `fuelConsumptionK`) and the UI / pit-stop menu (through `CarAvatar::physicsInfo`); the step never clamps `fuel` to it |
| `[FUELTANK] POSITION` = x,y,z | `fuelTankPos` | (0,0,0) | m, body frame | position of the tank body relative to the body origin |
| `[RIDE] PICKUP_FRONT_HEIGHT` | `ridePickupPoint[0]` = (0, value, z of suspension 0) | 0 | m | ride-height probe; read by `Car::computeRideHeight` @ `0x14026fc90` (only caller `Car::getPhysicsState`) and by `RaceEngineer::evalFrontRideHeight` / `evalRearRideHeight` (called by `getPhysicsState`, and through `evalRideHeight` by `SetupManager::isSetupRespectingRules`, the `[RULES] MIN_HEIGHT` check); no force uses it |
| `[RIDE] PICKUP_REAR_HEIGHT` | `ridePickupPoint[1]` = (0, value, z of suspension 2) | 0 | m | same |
| `[PIT_STOP] TYRE_CHANGE_TIME_SEC`, `FUEL_LITER_TIME_SEC`, `BODY_REPAIR_TIME_SEC`, `ENGINE_REPAIR_TIME_SEC`, `SUSP_REPAIR_TIME_SEC` | `pitTimings.*` | constructor 10 / 0.1 / 2 / 2 / 2; read only if the section exists | s | `Car::initPitstopTimings` @ `0x140274020`; used by `Car::getPitstopTime` @ `0x140271e60` |
| `[RULES] MIN_HEIGHT` | `setupManager.minimumHeight_m` | −1 | m | `SetupManager::init`; 27 of 113 cars |

Keys in `car.ini` that the **physics does not read**:

| Key | Who reads it |
|---|---|
| `[HEADER] VERSION` (1 in 74 cars, 2 in 38, `extended-2` in 1) | `CarAvatar::initCommon` @ `0x1400d56e0` and other graphics code. `Car::initCarData` has **no version switch** |
| `[BASIC] GRAPHICS_OFFSET`, `GRAPHICS_PITCH_ROTATION` (deg × 0.017453) | `CarAvatar::initCommon`. They move the 3D model relative to the body origin, **and through `makeBodyMatrix` they also place the collider mesh** (§5.5) |
| `[GRAPHICS] …` | cameras, mirrors, fuel warning light |
| `[INER_CALC]` | no code refers to this string, and no car in `cardata/` has the section |

### 4.2 Other files

| File | Key | Read by | Notes |
|---|---|---|---|
| `colliders.ini` (113 cars) | `[COLLIDER_n] CENTRE`, `SIZE` for n = 0,1,… until a section is missing | `CarColliderManager::loadINI` @ `0x1402a37a0` | metres, body frame. 107 cars also write `GROUND_ENABLE=1`; **no code reads `GROUND_ENABLE`** (nor the `[HEADER] COLLIDERS` count some cars have) |
| `content/cars/<name>/collider.kn5` | first mesh of the first node | `CarAvatar::initPhysics` @ `0x1400d7660` for local cars; `NetCarStateProvider::NetCarStateProvider` @ `0x140116c70` loads the same file for remote cars | graphics-side loader; see §5.5 |
| `fuel_cons.ini` (113 cars) | `[FUEL_EVAL] KM_PER_LITER` | `Car::Car` (path built from `unixName`, **not** through `getConfigPath`) | `expectedFuelPerLap = splineLength × (1 / (KM_PER_LITER × 1000))` litres, only when the section exists **and** `AISplineRecorder::getBestLapSpline(track->aiSplineRecorder)` is non-null; if the result is still 0 it becomes `RaceEngineer::evaluateFuelPerLapFromTrackSpline()`. Only for AI/pit strategy and the UI, not for consumption |
| `suspensions.ini` | `[BASIC] WHEELBASE`, `CG_LOCATION`; `[FRONT]/[REAR] TRACK`, `BASEY`, `HUB_MASS`; `[REAR] TYPE`; `[AXLE] TORQUE_REACTION` | suspension classes (`docs/map/suspension.md`); `TYPE`/`TORQUE_REACTION` in `Car::Car` | decides where the wheels sit relative to the body origin, see §5.1 |
| `suspensions.ini` | `[DAMAGE] MIN_VELOCITY`, `GAIN`, `MAX_DAMAGE`, `DEBUG_LOG` (110 of 113 cars; usual values 40 / 0.0004 / 0.05 / 1) → `SusDamageDef::minVelocity`, `damageGain`, `maxDamage`, `isDebug` | `Suspension::loadINI` @ `0x1402c1d90` and `SuspensionStrut::loadINI` @ `0x1402c4ee0`, only if the section exists. `SuspensionML::loadINI` @ `0x1402c9970` and `SuspensionAxle::SuspensionAxle` @ `0x1402c6b90` (the 4 rigid-axle cars) do not reference the section: their `damageData` keeps the constructor values (`minVelocity` 15, `damageGain` 0, `maxDamage` 0), so a multilink or rigid-axle corner never accumulates collision damage through this path (see suspension.md for what the axle does with damage) | **the collision thresholds of the suspension damage** (§5.6): `MIN_VELOCITY` is compared with the mean of two zone levels, i.e. km/h × `mechanicalDamageRate` |
| `damage.ini` (113 cars) | all keys | `VisualDamageManager::VisualDamageManager` @ `0x1401d2890` only | **graphics only** (bumpers hanging, glass, scratches). The physics never opens it |
| `aero.ini` | per wing `ZONE_<FRONT|REAR|LEFT|RIGHT>_CL` and `…_CD` (key names assembled from string pieces; only when aero.ini `[HEADER] VERSION >= 2`) → `Wing::damageCL[0..3]`, `damageCD[0..3]`. The arrays have 5 entries and a fifth name `CENTER` is built, but the read loop runs 4 times: no `ZONE_CENTER_*` key is read and index 4 stays unset (see aero.md 4.1). `Wing::SPEED_DAMAGE_COEFF` and `SURFACE_DAMAGE_COEFF` are **not ini keys**: both are the constant 300.0 written in the constructor (no string of that name exists in the executable) | `Wing::Wing` @ `0x1402b1340` | aero map; listed because it is how body damage reaches the aero |
| `cfg/race.ini` (Documents) | `[CAR_n] BALLAST`, `[CAR_n] RESTRICTOR` (section name built as `CAR_` + car index) | `RaceManager::initOffline` @ `0x14013a6c0` | → `CarAvatar::setBallastKG` @ `0x1400d9de0`, `CarAvatar::setRestrictor` @ `0x1400da6b0`. Online the server sends them (`ACClient::onMessageTCP` @ `0x14004c840`) |
| `system/data/surfaces.ini`, `content/tracks/<t>/data/surfaces.ini` | `[SURFACE_n] …`, `IS_PITLANE` | `SurfacesManager::loadSurfaceDefinitions` @ `0x1401afad0` | every surface loaded from a file gets `collisionCategory = 1`; the built-in `WALL` surface (`SurfacesManager::SurfacesManager` @ `0x1401ae9b0`) gets 2 |

---

## 5. Each step: inputs, maths, outputs

### 5.1 Mass, inertia and where the centre of gravity is

`IRigidBody::setMassBox(m, lx, ly, lz)` → `RigidBodyODE::setMassBox` @ `0x1402ce890` →
`dMassSetBoxTotal` @ `0x140346bc0` → `dBodySetMass` @ `0x14033fb30`. In this build *(disassembly)*:

```
mass = m                                  (stored as is, no density round trip)
k    = m * 0.083333336f                   (1/12 as a float, 0x3daaaaab)
Ixx  = (lz*lz + ly*ly) * k
Iyy  = (lx*lx + lz*lz) * k
Izz  = (lx*lx + ly*ly) * k                off-diagonal terms 0, centre of mass (0,0,0)
```

Sequence of mass writes:

1. `Car::initCarData`: `body->setMassBox(TOTALMASS, INERTIA.x, INERTIA.y, INERTIA.z)` (the hubs do
   not exist yet), `fuelTankBody->setMassBox(1.0, 0.5, 0.5, 0.5)`.
2. Each suspension gives its hub its own mass (`HUB_MASS`; `docs/map/suspension.md`).
3. End of `Car::Car` and then at most once per second — `Car::updateBodyMass` @ `0x140276c70`
   *(disassembly)*:

```
if (ksPhysics->physicsTime - lastBodyMassUpdateTime > 1000.0)          // double, milliseconds
    if (bodyInertia == (0,0,0))                                         // only with [EXPLICIT_INERTIA]
        body->setMassExplicitInertia(mass, explicitInertia.x, .y, .z);  printf("explicit")
    else
        body->setMassBox(calcBodyMass(), bodyInertia.x, .y, .z)
    fuelTankBody->setMassBox( max((float)((double)fuelKG * fuel), 0.1f), 0.5, 0.5, 0.5 )
    lastBodyMassUpdateTime = ksPhysics->physicsTime

calcBodyMass() = (mass - (m0 + m1 + m2 + m3)) + ballastKG       // float; m_i = suspensions[i]->getMass()
```

`lastBodyMassUpdateTime` starts at −1e8, so the first call (inside the constructor) always acts.
`ISuspension::getMass` (+0x90) is the **ODE mass of the hub body** (for a strut: the hub only, 80 % of `HUB_MASS`, not the strut body) (`Suspension::getMass`
@ `0x1402c4d30`); for a live axle it is half the axle body's mass (`SuspensionAxle::getMass`
@ `0x1402c84c0`).

Answers to the briefing's questions:

- **What TOTALMASS includes.** For DWB, ML and AXLE corners the simulated total is body + hubs
  (or axle) + tank = `TOTALMASS + ballastKG + fuelKG × fuel` (with a 0.1 kg floor for the tank).
  **Strut corners add mass:** `SuspensionStrut::SuspensionStrut` @ `0x1402c38d0` gives the hub
  `0.8 × HUB_MASS` and a separate strut body `0.2 × HUB_MASS`, but the strut's `getMass` is the
  same code as `Suspension::getMass` @ `0x1402c4d30` (hub only), so `calcBodyMass` subtracts only
  `0.8 × HUB_MASS` and the ODE total is `0.2 × HUB_MASS` heavier than that formula for every
  strut corner. `Car::getTotalMass` @ `0x140272570` and `powerClassIndex` do not count the strut
  bodies either. So `TOTALMASS` is the car with hubs/wheels and without fuel only on a car
  without struts. The driver is not a separate quantity; per Kunos'
  comment it is meant to be inside `TOTALMASS`.
- **Inertia.** Only the body mass feeds the box formula, so ballast and hub masses change the
  inertia proportionally; the box dimensions never change. The tank is a 0.5 m cube whose mass
  follows the fuel.
- **Centre of gravity.** ODE requires the centre of mass of a body to be its origin, so the
  **body origin is the centre of gravity of the sprung body**. The wheels are placed around it
  by `suspensions.ini` (`Suspension::loadINI` @ `0x1402c1d90`): front axle at
  `z = (1 − CG_LOCATION) × WHEELBASE`, rear axle at `z = −CG_LOCATION × WHEELBASE`,
  `x = ±TRACK/2`, `y = BASEY`. `CG_LOCATION` is therefore the front weight fraction of the body
  alone. The hubs and the fuel tank are separate bodies, so the centre of gravity of the whole
  car is not exactly the origin and moves as fuel burns (interpretation; the game never computes
  a whole-car CG). `Car::getCGHeight` @ `0x1402703b0` is simply body origin height minus the mean
  height of the four tyre contact points.
- **Ballast** (`Car::setBallastKG`): added to the body mass at the body origin on the next
  `updateBodyMass` (up to 1 s later). **Restrictor** (`Car::setRestrictor` @ `0x140275d10`):
  `acEngine.restrictor = clamp(value × 0.0025, 0, 1)`; it is an engine input (`Engine::step`
  @ `0x1402880e0` reduces the air term by `restrictor × rpm × 0.0001 × …`), it has nothing to do
  with mass. `Car::getRestrictor` @ `0x140272140` returns `roundf(restrictor × 400)`.

**The `[EXPLICIT_INERTIA]` path is broken** *(disassembly of `RigidBodyODE::setMassExplicitInertia`
@ `0x1402ce910`)*: it writes the three values to byte offsets 0x14, 0x24 and 0x34 of ODE's mass
record. The box path and `RigidBodyODE::getLocalInertia` @ `0x1402ce480` use offsets 0x14, 0x28 and
0x3c, which are the real diagonal of ODE's 3×4 inertia matrix. So the explicit path fills the
**first column** (as if the matrix were 3×3), leaving the diagonal entries for y and z at zero.
`dBodySetMass` then falls back to an identity inverse inertia when `_dInvertPDMatrix`
@ `0x14034bac0` rejects the matrix (the fallback call to `dRSetIdentity` @ `0x1403465f0` is in the
disassembly; that the rejection really happens is my reading of the ODE 0.13.1 source). Two more
differences of that path: the mass is `mass` (hubs **not** subtracted, ballast ignored), and it
prints a line every second. No stock car uses the section, so this is only a trap for mods.

### 5.2 Fuel

In `Car::step` *(disassembly)*, every step, every car:

```
boost = (turboBoost >= 0) ? (double)turboBoost : 0.0          // drivetrain.acEngine.status.turboBoost
f     = fabsf(Drivetrain::getEngineRPM()) * dt * acEngine.gasUsage          // float multiplies
fuel  = fuel - (double)f * (boost + 1.0) * fuelConsumptionK * 0.001 * (double)ksPhysics->fuelConsumptionRate
if (fuel > 0)  acEngine.fuelPressure = 1.0
else           fuel = 0;  acEngine.fuelPressure = 0.0         // engine starves
Car::updateBodyMass()
```

- `fuel`, `fuelConsumptionK` are doubles; the product is evaluated left to right in double.
- `gasUsage` is an `Engine` member written by `Engine::step` @ `0x1402880e0`:
  `gasUsage = electronicOverride × gasInput`, i.e. the throttle the engine really used that step
  (after the limiter cut and electronics; details in the drivetrain map). `fuelConsumptionRate`
  is the session multiplier (1.0 in the `PhysicsEngine` constructor; 0 means no consumption).
- `fuelPressure` is not owned by the body alone: `Car::step` sets it to 1 or 0 every step, and
  `Engine::step` (later in the same step) sets it to 0 again while `lifeLeft ≤ 0` (blown engine).
- The tank mass only follows once per second (`updateBodyMass`), not every step.
- `Car::reset` @ `0x1402758e0` (called by `forcePosition`) sets `fuel = requestedFuel`, i.e. a
  teleport refuels. `Car::setRequestedFuel(v, keep)` sets `fuel` (and `requestedFuel` if `keep`).
  `Car::setFuelForLaps(laps, mult)` @ `0x140275b60` *(disassembly)*: `perLap` =
  `FuelLapEvaluator::getFuelPerLap()` or, while that is ≤ 0, `expectedFuelPerLap`;
  `want = (float)laps × perLap × mult`. If `want ≥ maxFuel` the fuel is **left unchanged** and the
  function returns `(int)(maxFuel / perLap)` (the laps that fit; `AIDriver` passes `mult` 1.1 or
  1.2 and mostly ignores the return value);
  otherwise `fuel = requestedFuel = max(want, 0)` and it returns `laps`. It is not a clamp to
  `maxFuel`.
- `Car::getTotalMass(withFuel)` @ `0x140272570` = body mass + four `getMass()` (+ `(float)fuel × fuelKG`).

### 5.3 Sleeping rule

In `Car::step` *(disassembly)*, after `Autoclutch::step`:

```
allLoaded = every tyre has status.load > 0
if (valueCache.speed >= 0.5)                       -> sleepingFrames = 0        // m/s
else if (|body angular velocity|^2 >= 1.0)         -> sleepingFrames = 0
else if (gas > 0.01 && clutch > 0.01 && drivetrain.currentGear != 1) -> sleepingFrames = 0
else if (!allLoaded)                               -> sleepingFrames = 0
else                                                  sleepingFrames += 1
if (sleepingFrames > framesToSleep)                // framesToSleep = 50
    body->stop(0.99);  fuelTankBody->stop(0.99)
```

`RigidBodyODE::stop` @ `0x1402cead0` ignores its argument and zeroes linear velocity, angular
velocity, accumulated force and accumulated torque. The hubs are not stopped here. The call is
repeated every step while the counter stays above the limit, and it happens before
`stepComponents` and `dWorldStep`, so a sleeping car still receives that step's tyre, spring and
gravity forces and is then zeroed again one step later (interpretation of the order, not measured). `Car::isSleeping` @ `0x1402745e0`
(`framesToSleep < sleepingFrames`) is called only by `Tyre::stepRotationMatrix` @ `0x140284b80`;
`Car::getPhysicsState` @ `0x140270d70` makes the same comparison on the two members directly.

Also in `Car::step`, for the car with `physicsGUID == 0` only: while `|v|² ≥ 1` every suspension
gets `setERPCFM(0.3, suspension->baseCFM)`, otherwise `setERPCFM(0.9, 1e-7)`; the fuel-tank joint
gets the same ERP and CFM −1 (ignored). Other cars keep the joint settings they were created with.

Body-frame acceleration, every step: `accG = body->worldToLocalNormal((v − lastVelocity) × (1/dt) × 0.10197838)`,
then `lastVelocity = v` (0.10197838 = 1/9.806).

### 5.4 Coolant temperature (`ThermalObject`)

`Car::stepThermalObjects` @ `0x1402769f0`, then `ThermalObject::step` @ `0x1402b2f00`:

```
rpm = Drivetrain::getEngineRPM()
if (rpm > acEngine.data.minimum * 0.8)
    water.heatAccumulator += (rpm / Engine::getLimiterRPM()) * 20 * controls.gas + 85
h = water.heatAccumulator;  water.heatAccumulator = 0
t = ((1 - coolSpeedK * speed) * ambientTemperature - t) * (1/tmass) * dt * coolFactor + t
if (h != 0)  t = (h - t) * (1/tmass) * dt * heatFactor + t
```

`tmass = 20`, `coolSpeedK = 0.002`, `coolFactor = 0.2`, `heatFactor = 1.0`; `t` starts at 0 and is
set to 60 by `Car::reset`. Only `Car::getPhysicsState` reads `water.t`: it is a display value and
has no effect on the engine.

### 5.5 Colliders and collision filtering

Two kinds of shape hang on `Car::body`, both in the car's own dynamic sub-space
(`physicsGUID + 1`, `PhysicsCore::getDynamicSubSpace` @ `0x1402cc920`):

- **Floor boxes** — `CarColliderManager::loadINI` *(disassembly of the call)*:
  `body->addBoxCollider(&CENTRE, &SIZE, category = 4, mask = 1, space = physicsGUID + 1)`
  (`RigidBodyODE::addBoxCollider` @ `0x1402cddb0`: `dCreateBox(SIZE)`, offset position `CENTRE`).
- **Collider mesh** — `Car::initColliderMesh` @ `0x140273b20`: copies the positions of the mesh
  vertices (0x2c-byte vertices → 12-byte vec3), computes `Car::bounds`, then
  `body->addMeshCollider(vertices, count, indices, indexCount, matrix, category = 4, mask = 0x1e, space = physicsGUID + 1)`.
  `matrix` is identity translated by `GRAPHICS_OFFSET` and pitched by `GRAPHICS_PITCH_ROTATION`
  (`CarAvatar::makeBodyMatrix` @ `0x1400d8ec0`): the collider file is authored in model space.

Category and mask bits (constants found in the callers):

| Shape | Category | Collide mask | Where |
|---|---|---|---|
| Track surface listed in `surfaces.ini` | 1 | 0x14 | `Track::addSurface` @ `0x140277e50` passes `surfaceDef->collisionCategory` and the literal 0x14 |
| Track mesh matched to the built-in `WALL` surface | 2 | 0x14 | same; `TrackAvatar::addPhysicsMesh` @ `0x1401c78e0` adds 10000 to the static sub-space id of category-2 meshes |
| Car floor box | 4 | 1 | `CarColliderManager::loadINI` |
| Car collider mesh | 4 | 0x1e, changed every step (below) | `Car::initColliderMesh`, `Car::updateColliderStatus` |
| Remote (network) car mesh | 8, or 0 while ghosted | 4; 0 if the remote car's `unixName` is `spectator` *(disassembly of both call sites)* | `NetCarStateProvider::NetCarStateProvider` @ `0x140116c70`, `NetCarStateProvider::step` @ `0x14011bd00` |
| Loose track object | 0x10 | 0x1f while its body is enabled, 0x0c otherwise | `PhysicsObject::PhysicsObject` @ `0x1402ac8a0` and its step lambda at `0x1402acb00` |
| Tyre / AI rays | not filtered | not filtered | `rayNearCallback` @ `0x1402cd210` tests no bits; it only skips geoms that have a body |

Two shapes are tested only if **both** `catA & maskB` and `catB & maskA` are non-zero
(`nearCallback` @ `0x1402ccc70`). The resulting pairs:

| Pair | Collide? |
|---|---|
| floor box ↔ track surface (1) | yes — the only thing a box ever touches |
| floor box ↔ wall, other car, object | no |
| collider mesh ↔ wall (2) | yes |
| collider mesh ↔ track surface (1) | only while the car is rolled over (mask value 1 = bit 0 added) |
| collider mesh ↔ other local car's mesh | yes, unless either car has pit-lane ghosting |
| collider mesh ↔ remote car (8) | yes, unless the remote car's category is 0 |
| collider mesh ↔ loose object (0x10) | yes |
| shapes of the same car | never (same sub-space; interpretation of how `dSpaceCollide` recurses) |

`Car::updateColliderStatus` @ `0x140276df0`, end of every `Car::step`:

```
if (any tyre's surfaceDef != null && surfaceDef->isPitlane)   isCollisionOffForPits = true
else if (isCollisionOffForPits)
    if (no other car's body origin within 6 m (dist^2 < 36))   isCollisionOffForPits = false
mask = (unixName == "spectator") ? 0x02 : (isCollisionOffForPits ? 0x1a : 0x1e)
if (body world matrix M22 < 0.25)  mask |= 1          // body "up" axis more than ~75.5 deg from vertical
body->setMeshCollideMask(0, mask)                     // RigidBodyODE::setMeshCollideMask 0x1402ce9c0
```

- 0x1a is 0x1e without the value 4 (bit 2, the local-car category): the mesh stops colliding with other local cars, still with walls,
  remote cars and loose objects.
- `RigidBodyODE::setMeshCollideMask` has **no bounds check** *(disassembly)*: a body without a
  mesh collider makes this a null read. `CarAvatar::initPhysics` only adds the mesh if
  `content/cars/<name>/collider.kn5` exists. Counted in the installed game (read only): all 112
  car folders that contain a data pack also contain `collider.kn5` (the other 70 folders are
  stubs without data), and so does the SDK example car.
- Remote cars: `NetCarStateProvider::step` sets the remote mesh category to 0 when the remote car
  is disconnected, when the local player (`Sim::getCar(0)`) is in the pit lane or when the last
  network packet is older than 5000 ms, and back to 8 once the two body origins are more than
  6 m apart.
- There is no separate "collisions off" switch in `Car`; the pit rule, the `spectator` name and
  the remote-car category are the only mechanisms found.
- `PhysicsCore::setNoCollisionSteps` @ `0x1402cd660`: the only caller found is the new-session
  handler in `PhysicsAvatar` (lambda at `0x1401213f0`), which skips collision detection for 250
  steps (0.75 s) at the start of every session. Teleports do not call it.

### 5.6 Collision callback and damage

`PhysicsCore::onCollision` @ `0x1402ccda0` calls, once per contact point that was turned into a
contact joint, `collisionCallback->onCollisionCallBack(bodyA, shapeA, bodyB, shapeB, normal, pos, depth)`
where `body*` is the `IRigidBody` stored as ODE body data (null for the static track), `shape*`
is the `ICollisionObject` stored as geom data (null for boxes). A is the first geom ODE handed
to `nearCallback`, B the second, and `normal` is ODE's contact normal for that order.
Two things decide how often this fires (both in `docs/map/physics_engine.md`, confirmed here in
`PhysicsCore::collisionStep` @ `0x1402cbf90` and `onCollision`): each step runs only **one** of
the two collision passes (even frames: dynamic against dynamic = car–car and car–object; odd
frames: dynamic against static = car–track and car–wall), so a given contact reports on every
second step; and a floor-box contact (ODE class box against class trimesh, either order) is
dropped unless its normal, expressed in the frame of the first geom's body (or the second's, if
the first has none), has `y ≥ 0.9` — dropped means **no contact joint and no callback**.
`PhysicsEngine::onCollisionCallBack` @ `0x140264020` swaps A and B if A is not a car body but B
is, then calls `Car::onCollisionCallBack` @ `0x140274650` on **every** car. Each car returns at
once unless one of the two bodies is its own `body` (hub and tank contacts cannot happen: they
have no shapes). Then *(disassembly)*:

```
other      = the body that is not mine (may be null = track)
groundA    = shapeA != null && shapeA->getGroup() in {1, 0x10}
groundB    = shapeB != null && shapeB->getGroup() in {1, 0x10}
lastCollisionTime = physicsTime
local      = body->worldToLocal(pos)
vOther     = other ? other->getPointVelocity(pos) : 0
vMine      = body->getPointVelocity(pos)
relSpeed   = -( ((vMine.y-vOther.y)*n.y + (vMine.x-vOther.x)*n.x + (vMine.z-vOther.z)*n.z) * 3.6 )    // km/h
if (bodyA && bodyB && !groundA && !groundB)   lastCollisionWithCarTime = physicsTime

if (relSpeed > 0 && !groundA && !groundB)
    d = relSpeed * ksPhysics->mechanicalDamageRate
    if (d > 150)  Engine::blowUp()                       // acEngine.lifeLeft = -100
    dir = normalize(local.x, 0, local.z)
    if (|dir.z| > 0.707)   zone = (local.z > 0) ? 0 : 1          // 0 front, 1 rear
    else                   zone = (local.x >= 0) ? 2 : 3         // 2 = left (+x, wheels 0 and 2), 3 = right (wheels 1 and 3)
    damageZoneLevel[zone] = max(damageZoneLevel[zone], d)
    damageZoneLevel[4]    = max(damageZoneLevel[4],    d)          // overall

// every call, even for ground contacts:
if (dz[0] > 0 && dz[2] > 0)  suspensions[0]->setDamage((dz[0] + dz[2]) * 0.5)
if (dz[0] > 0 && dz[3] > 0)  suspensions[1]->setDamage((dz[0] + dz[3]) * 0.5)
if (dz[1] > 0 && dz[2] > 0)  suspensions[2]->setDamage((dz[1] + dz[2]) * 0.5)
if (dz[1] > 0 && dz[3] > 0)  suspensions[3]->setDamage((dz[1] + dz[3]) * 0.5)

eventQueue.push(ACPhysicsEvent{ type 0, param1 = (float)physicsGUID, param2 = depth, param3 = -1,
                                param4 = relSpeed, vParam1 = pos, vParam2 = normal,
                                ulParam0 = shapeB->getGroup() })       // shapeB is not null-tested
if (relSpeed > 0 && !groundA && !groundB)
    evOnCollisionEvent(OnCollisionEvent{ other, relSpeed, pos, local, shapeB->getGroup() })
```

- Damage is a **closing speed in km/h**, not an impulse. A zone keeps the largest value seen; it
  does not add up. With `mechanicalDamageRate = 0` nothing is ever recorded.
- **The normal is never flipped per car.** `PhysicsEngine::onCollisionCallBack` swaps the two
  bodies and shapes when only B is a local car, but passes the same `normal`, and every car
  computes `(vMine − vOther)·n` with that one vector. In a contact between two local cars the
  two calls therefore get exactly opposite `relSpeed` (same point, same normal): for one contact
  point only **one** of the two cars passes `relSpeed > 0` and records damage / fires
  `evOnCollisionEvent`; the other pushes an event with a negative `param4`. Which car it is
  depends on ODE's geom order and normal convention (by the ODE convention the normal points
  into the first geom, which would make it the first car while closing and the second while
  separating; not verified inside ODE, see Open questions).
- `shapeB->getGroup()` is safe in practice: a shape is null only for a box, a box is only ever
  paired with a track surface (masks, §5.5), and `PhysicsEngine::onCollisionCallBack` has already
  swapped the pair so that the local car's body (with its box) is A and the track mesh is B. This
  does not depend on the order in which ODE hands over the two geoms.
- "Ground" means track surface (category 1) or loose object (0x10). Walls (2), cars (4) and
  remote cars (8) damage. Floor-box contacts therefore never damage; they still push an event,
  which `Sim::stepPhysicsEvent` hands to `CarAudioFMOD::onCarHit` (the scrape/hit sound).
- `Suspension::setDamage(v)` @ `0x1402c31e0`: if `v > minVelocity`,
  `damageAmount = min((v − minVelocity) × damageGain, maxDamage)`. `getDamage()` (+0x88) returns
  `damageAmount / maxDamage` (0 when `maxDamage` is 0). For `SuspensionAxle` slots +0x78 and
  +0x80 are the shared empty function and +0x88 is a shared stub that returns 0.0: a live
  axle takes no collision damage (`SuspensionStrut` and `SuspensionML` have their own
  implementations; suspension map).
- Repair: `Car::setDamageLevel(0)` zeroes all five numbers, `Car::resetSuspensionDamageLevel`
  calls `resetDamage` (+0x80) on each suspension. Callers: `CarAvatar::setDamageLevel`
  @ `0x1400da020`, two pit-stop lambdas, the teleport lambda at `0x1400d0440` and
  `AIDriver::stepSplineSelection` (`resetSuspensionDamageLevel` itself is called only by the last
  two). Engine life (`lifeLeft`) is not touched by these two functions, but it is restored
  elsewhere: `Engine::reset` @ `0x140287f90` sets `lifeLeft = 1000` (and resets the turbos), and
  it is called by `Drivetrain::reset` @ `0x140269280`, i.e. **on every `Car::forcePosition`**, and
  by the pit-stop lambdas. The two pit-stop lambdas (`0x1400a7ac0`, `0x1400a7da0`) do the three
  repairs separately, each behind its own menu tick: engine → `Engine::reset`; body →
  `setDamageLevel(0)`; suspension → `setDamageLevel(0)` plus `resetDamage` (+0x80) on every
  suspension (inlined loop). So a body-only repair leaves `damageAmount` of the suspensions alone,
  and because `onCollisionCallBack` only re-sends `setDamage` while two zone levels are > 0, it
  stays at its old value.
- `Car::getPitstopTime` @ `0x140271e60`: body repair time per zone 0–3 with `dz > 0` =
  `(bodyRepairTimeSec − 2) × clamp(dz × 0.01, 0, 1) + 2`; engine (only if `lifeLeft < 1000`) =
  `(1000 − lifeLeft) × engineRepairTimeSec × 0.1`; suspension =
  `clamp(getDamage(), 0, 1) × suspRepairTimeSec` per corner with `getDamage() > 0`
  (`getDamage` = `ISuspension` +0x88). If the last argument is set, every term (and the tyre and
  fuel times) also gets the same random `rand() × 3.051851e-05` (0…1 s) added. Rules, not physics.

### 5.7 Teleporting

`Car::forceRotation(d)` @ `0x140270040`: builds a matrix with rows
`normalize(−d.z, 0, d.x)`, `(0, 1, 0)`, `(−d.x, −d.y, −d.z)`, sets it on `body` and
`fuelTankBody` (`setRotation`, +0x98), calls `attach()` (+0x40) on every suspension, stops both
bodies. The body's z row becomes `−d`, so the argument is **minus the wanted body +z axis**
(the front axle sits at +z, §5.1, so `d` points backwards): `Car::step` passes
`−pitPosition.M31..M33`, which makes the body z row equal to the pit matrix z row.

`Car::forcePosition(p, invalidateLap)` @ `0x14026fe10`:

```
p.y += RaceEngineer::getBaseCarHeight() + 0.01     // max over front/rear of (tyre radius - suspension base y)
Car::reset()                                        // fuel = requestedFuel, water.t = 60, penalties 0, KERS/ERS reset,
                                                    // isCollisionOffForPits = false, framesToSleep = 50
body->stop(); body->setPosition(p)
fuelTankBody->setPosition(body->localToWorld(fuelTankPos))
each suspension: stop() (+0x98), attach() (+0x40)   // hubs back to their design position
Drivetrain::reset (-> Engine::reset: lifeLeft = 1000, a teleport repairs the engine),
BrakeSystem::reset, Tyre::reset x4, Drivetrain::setCurrentGear(1, true)
if (invalidateLap) TimeTransponder::invalidate
evOnForcedPositionCompleted(p)
body->stop(); fuelTankBody->stop(); framesToSleep = 50; SplineLocator::reset
```

Callers: `CarAvatar::forcePosition` @ `0x1400d2f30` (lambda at `0x1400d0440`, which also repairs
damage), `Car::step` for a black-flagged car outside its pit box, `PenaltyManager::addJumpStartPenalty`,
`AIDriver`. In `Car::step` the order is `forceRotation`, then `forcePosition`. Neither function
changes collision state beyond clearing `isCollisionOffForPits`.

---

## 6. Connections to other systems

Inputs written by others:

| Member | Written by | When |
|---|---|---|
| `drivetrain.acEngine.status.turboBoost`, `acEngine.gasUsage`, engine rpm | `Engine::step` (inside `Drivetrain::step`, in `stepComponents`) | previous step: fuel is burned **before** `stepComponents` |
| `tyres[i].status.load`, `tyres[i].surfaceDef` | `Tyre::step` (in `stepComponents`) | previous step's values feed the sleeping rule (before) ; this step's `surfaceDef` feeds `updateColliderStatus` (after) |
| `controls.gas`, `controls.clutch` | `Car::pollControls`, then the lock / gentle-stop overrides, then `Autoclutch::step`, which rewrites `clutch` immediately before the sleeping rule | same step |
| `drivetrain.currentGear` | `GearChanger::step` / `Drivetrain::step` in `stepComponents` | **previous step** (they run after the sleeping rule) |
| `valueCache.speed` | `Car::stepPreCacheValues` | before `Car::step` |
| `ballastKG`, `acEngine.restrictor`, `fuel`, `requestedFuel`, `damageZoneLevel` (repair) | `CarAvatar` lambdas, `ACClient`, `AIDriver`, pit stop | between steps, on the physics thread |
| `ksPhysics->mechanicalDamageRate`, `fuelConsumptionRate`, `ambientTemperature` | race / server settings | session start |
| hub masses | suspension constructors | car build |

Outputs read by others:

| Output | Read by |
|---|---|
| mass and inertia of `body`, `fuelTankBody` (ODE) | `dWorldStep`; `Car::getTotalMass` → `RaceEngineer`, `AIDriver`; `powerClassIndex` |
| `fuel` → `acEngine.fuelPressure` (0 or 1) | engine torque (drivetrain map) |
| `damageZoneLevel[0..3]` | `Wing::addDrag` @ `0x1402b2420` and `Wing::addLift` @ `0x1402b2730`: when the wing `hasDamage` and `mechanicalDamageRate > 0`, the worst over the four zones of `SURFACE_DAMAGE_COEFF × damageCD/CL[zone] × (damageZoneLevel[zone] / SPEED_DAMAGE_COEFF)²`, clamped to [0,1], degrades the wing (called from `AeroMap::step` in `stepComponents`); `DriftModeComponent::validateDrift` @ `0x1402c0270`; `Car::getPitstopTime`; `Car::getPhysicsState` (HUD, shared memory, replay, network damage report, visual damage); the automated test mode inside `PhysicsEngine::step` @ `0x140264760` (`isTestMode`, AI test: any of the five levels of car 0 above 0 writes `FAIL|DAMAGE|` and exits the process) |
| `damageZoneLevel[4]` | display / network only |
| suspension `damageData.damageAmount` | suspension geometry (`docs/map/suspension.md`) |
| `acEngine.lifeLeft = −100` (blow-up) | engine |
| `sleepingFrames` / `isSleeping()` | `Tyre::stepRotationMatrix`, `Car::getPhysicsState` |
| `accG` | `DRS::step`, `EDL::step`, `DynamicController::getInput` and `DynamicWingController::getInput` (controller inputs), `Car::pollControls`, plus cameras, driver animation, telemetry, shared memory |
| `lastCollisionTime`, `lastCollisionWithCarTime`, `evOnCollisionEvent` | `LapInvalidator::step`, `AIDriver`, `Sim`, `ACClient` (handlers registered on the event) |
| `ACPhysicsEvent` queue | `Sim::stepPhysicsEvent` @ `0x14019ebd0` → `CarAudioFMOD::onCarHit`; other consumers not traced |
| `isCollisionOffForPits` | only `updateColliderStatus` |
| `carHalfWidth` | `SplineLocator::step` (track-limit test) |
| `bounds` | written here; no reader found by member name in the decompiles |

Other mechanical damage that does **not** go through `damageZoneLevel` (interface only):
engine wear from over-rev and over-boost (`Engine::step`, scaled by `mechanicalDamageRate`),
gearbox shift window shrinking on mis-shifts (`Drivetrain::setCurrentGear` @ `0x1402692e0`), tyre
flat spots and punctures (`Tyre::stepFlatSpot` @ `0x140284500`, `Tyre::stepPuncture` @ `0x1402849b0`).

Position in the step order: fuel, mass refresh, sleeping, `accG` and coolant run in `Car::step`
**before** `Car::stepComponents`; `CarColliderManager::step` sits inside `stepComponents` after
`SpeedLimiter::step` and does nothing in normal play; `updateColliderStatus` runs after
`stepComponents`; the collision callback runs later, inside `core->step`, after all cars have
stepped; `Car::postStep` runs last from `evOnStepCompleted`.

---

## 7. Port notes

**Effort: M.** The body code itself is small (mass refresh, fuel, sleeping, a mask, one callback),
but none of it means anything without the rigid-body layer underneath it.

Must exist first: the ODE port stage 1 from `docs/map/physics_engine.md` (bodies, `dBodySetMass`,
fixed joint) for mass and fuel; stage 2/3 (contacts, box–mesh and mesh–mesh colliders) before the
collision callback can be exercised with real contacts. The suspension must exist because
`calcBodyMass` reads the hub masses, and the engine because fuel reads rpm, boost and `gasUsage`.

Things to copy exactly:

- `setMassBox` maths in float as in §5.1; the fuel product in double as in §5.2; the 1000 ms gate
  compares doubles of `physicsTime`.
- Evaluation order inside `Car::step` (pollControls → fuel → mass → control overrides →
  Autoclutch → sleeping → accG → coolant → components → mask), as in the tree of §3.2.
- Damage keeps maxima; suspension `setDamage` is re-sent on every callback.
- Do **not** "fix" the unread keys (`GROUND_ENABLE`, `[INER_CALC]`, `damage.ini`) or the explicit
  inertia path unless a mod car needs it.

Bit-exact testing with an oracle:

| What | Game functions to call | What must be faked |
|---|---|---|
| Mass / inertia | `ksPhysicsCoreODEFactory::create` @ `0x1402cb630`, `PhysicsCore::createRigidBody`, `RigidBodyODE::setMassBox`, `getMass`, `getLocalInertia` | nothing (real ODE in-process, as option (c) of the physics-engine map) |
| `updateBodyMass`, fuel, sleeping, `accG` | `Car::updateBodyMass`, or the head of `Car::step` | needs a whole `Car` (§9); cheaper alternative: a hand-built `Car` buffer with only `body`, `fuelTankBody`, `suspensions[4]` (fake objects with a `getMass` slot), `ksPhysics`, and the scalar members, calling `Car::updateBodyMass` and `Car::calcBodyMass` directly |
| Collision callback | `Car::onCollisionCallBack` directly with chosen bodies, shapes, normal, position | two real `RigidBodyODE` with set velocities; fake `ICollisionObject` (vtable with `getGroup` at +0x18); fake suspensions with `setDamage` at +0x78; a real `Concurrency::concurrent_queue` in the fake `PhysicsEngine`; a real `Engine` or accept the write to `lifeLeft` |
| Collider mask | `Car::updateColliderStatus` directly | body with one mesh collider; `tyres[i].surfaceDef`; `ksPhysics->cars` |
| Teleport | `Car::forceRotation`, `Car::forcePosition` | whole `Car` |

---

## 8. Open questions

1. **Explicit inertia consequence.** That `setMassExplicitInertia` writes the wrong matrix cells
   is confirmed from the disassembly; that ODE then rejects the matrix and uses an identity
   inverse inertia was inferred from the ODE 0.13.1 source (Cholesky reads the lower triangle),
   not re-read in `_dInvertPDMatrix`. One oracle call settles it. Chosen: document it as a bug and
   not port that path for now (0 of 113 cars).
2. **`physics_engine.md`, "category bits 0x30" — no longer a discrepancy.** The last call in
   `PhysicsCore::PhysicsCore` @ `0x1402cba80` is applied to `this->id`, which is the **world**,
   not the ray. Because `dGeomSetCategoryBits` and `dWorldSetQuickStepNumIterations`
   @ `0x1403404b0` share one address, the call is really "quick-step iterations = 48", which has
   no effect (the quick stepper is not used). The ray keeps ODE's default bits and
   `rayNearCallback` does not test bits anyway. The current `docs/map/physics_engine.md` (§3.1
   table) says the same; nothing to correct there any more.
3. **Per-step `setERPCFM` only for car 0 — agrees with `physics_engine.md` §3.4.** It runs only
   for the car with `physicsGUID == 0` (the first car created). Whether that is intended, and what
   ERP/CFM the other cars' joints end up with, belongs to the suspension map.
4. **`setNoCollisionSteps` — agrees with `physics_engine.md` §3.2.** It is not called after a
   teleport; the only caller found sets 250 steps at every new session.
5. **Which side is "left" — resolved by the review.** `Suspension::loadINI` @ `0x1402c1d90` puts
   wheels 0 and 2 at `x = +TRACK/2` and wheels 1 and 3 at `x = −TRACK/2`, and `Car::Car` passes
   `Left` for wheel 2 and `Right` for wheel 3 to `SuspensionAxle::SuspensionAxle`. So +x is the
   car's left, zone 2 (`local.x ≥ 0`) is the left side and its pairing with suspensions 0 and 2
   is geometrically consistent; zone 3 is the right side. Not cross-checked against the
   `damage_displayer` textures.
6. **`forceRotation` argument — direction resolved, one caveat left.** `d` is minus the wanted
   body +z axis (§5.7). The matrix is not orthonormal when `d` has a vertical component (row 2 is
   always (0,1,0), row 3 is `−d` unnormalised); what ODE does with such a matrix in
   `setRotation` was not checked.
7. **Surfaces with `collisionCategory` 0.** `SurfacesManager::getSurface` @ `0x1401af340` returns
   category 0 on some paths (surface name not found); such a mesh would collide with nothing but
   still be hit by rays. Not traced further (track-surface map).
8. **Remote-car mask — resolved by the review.** `NetCarStateProvider::NetCarStateProvider` has two
   `addMeshCollider` call sites (calls at `0x140117c6f` and `0x140117cb3`), chosen by the compare
   of `unixName` with `spectator`: category 8 in both; mask 4 for a normal car, mask 0 (a zeroed
   register) for a spectator. This is what the table in §5.5 states.
9. **Same-car shapes never collide** is my reading of how `dSpaceCollide` on a space of sub-spaces
   calls `nearCallback` (which only uses `dSpaceCollide2` between two different spaces). Not
   verified inside ODE.
10. **`Path::getPath(file) + ".acd"` — resolved by the review.** `Path::getPath` @ `0x140231bc0`
    returns everything before the last `/` or `\` (no trailing slash), so the archive name is
    `content/cars/<name>/data.acd` (or `data_<config>.acd` for a config override).
11. **How many stock cars ship `collider.kn5` — resolved by the review.** Recounted in the
    installed game (directory listing only): 182 car folders, 112 with `data.acd` or `data/`, and
    all 112 of those have `collider.kn5` (§5.5). A car without one would still crash in
    `Car::updateColliderStatus`; that only matters for mods and for the oracle (§9).
12. **Units of `aero.ini` damage keys** and the exact clamp/usage after the zone loop in
    `Wing::addLift`/`addDrag` are left to the aero map; only the read of `damageZoneLevel` is
    confirmed here.
13. **`Car::postStep` argument — confirmed, not an artefact.** The handler lambda at `0x14026ef00`
    is three instructions (`mov rcx,[rcx+8]; xorps xmm1,xmm1; jmp Car::postStep`), so `dt` really
    is 0.0 and `splineLocatorData.sideVelocity = (side − lastSide) / 0` is ±infinity or NaN every
    step. No reader of `sideVelocity` was found in the pseudo-C by member name (only the
    constructor and `postStep` touch it), so it looks like a dead value; a port should not
    "fix" it without checking the readers of `SplineLocatorData` copies (not body physics).
14. Choices made: `Car::getPhysicsState` (4.3 kB, the snapshot for graphics/telemetry) was not
    mapped field by field; the pit-stop timing rules, penalties, jump start and lights toggle that
    also live in `Car::step` are mentioned but left to the rules/electronics maps.

---

## 9. Building a Car outside the game

Goal: decide whether an oracle can construct a whole AC `Car` on a fake flat track. Short answer:
**yes, but it needs the real ODE core, a real (small) `Track` with an AI spline, a mesh collider
and a controls object; the constructor alone needs far less than the first `Car::step`.**

### 9.1 What `Car::Car` touches

Signature: `Car::Car(PhysicsEngine*, const std::wstring& unixName, const std::wstring& configName)`;
the caller allocates 0x3ea0 bytes.

| Need | Detail | Fake or real? |
|---|---|---|
| Process state | CRT and C++ runtime imports (`msvcr120`, `msvcp120`, incl. `Concurrency::details::_Concurrent_queue_base_v4`), working directory = a folder that contains `content/cars/<name>/data/` and `system/cfg/` | as in `tools/tyre_oracle` |
| `INIReader::useCache` (global, `0x14151d0f9`) | must be 0, or the static cache map must be constructed | write 0 (already done by the tyre oracle) |
| `FolderEncrypter::keys` (global) | only if a `data.acd` sits next to the `data/` folder | avoid: give the oracle an unpacked `data/` folder and no `data.acd` |
| `PhysicsEngine* ksPhysics` | read or written during construction: `cars` (GUID = size, `push_back`), `core`, `track`, `physicsTime`, `evOnNewSessionPhysics`, `evOnStepCompleted` (handlers appended), `slipStreams` (`push_back`), plus whatever the component inits read (`roadTemperature`, `ambientTemperature`, `mechanicalDamageRate`, `getAirDensity`, …) | simplest is the real `PhysicsEngine::PhysicsEngine` @ `0x140262430`: it reads `system/cfg/assetto_corsa.ini`, creates the real core, calls `ksGetTime`, and **starts a thread pool when the machine has ≥ 4 logical CPUs** unless `[PHYSICS_THREADING] THREADS=0` — put that in the oracle's own `assetto_corsa.ini` |
| `IPhysicsCore` | `createRigidBody` (+0x18) for body, tank, hubs, struts, axle; `createFixedJoint` (+0x80); distance / slider / ball joints from the suspensions; `createRayCaster` (+0x88) from the tyres via the track | **real** `PhysicsCore` (`ksPhysicsCoreODEFactory::create` @ `0x1402cb630`; runs `dInitODE2`, needs thread-local storage for ODE). Faking it means re-implementing ODE |
| `Track* ksPhysics->track` | must be non-null. Used as `IRayTrackCollisionProvider` by `Tyre::init`; `aiSplineRecorder` (`SplineLocator::init` @ `0x1402ab2a0`, fuel estimate), `isOpen` and `timeLines` (`TimeTransponder::init` @ `0x1402907c0`), `dataFolder` (`RaceEngineer::evaluateFuelPerLapFromTrackSpline`: a missing `ideal_line.ai` is handled and returns 30) | real `Track::Track` @ `0x140277100` (physics library, no graphics; reads `cfg/race.ini` through `INIReaderDocuments` and `<track>/data/drs_zones.ini`), then `Track::addSurface` @ `0x140277e50` with two triangles and a `SurfaceDef` (category 1), then `Track::initAISpline` @ `0x1402782a0` |
| Car data files | from `content/cars/<name>/data/` (or `data_<config>/`): `car.ini`, `suspensions.ini`, `tyres.ini` and its `.lut` files, `brakes.ini`, `drivetrain.ini`, `engine.ini` + `power.lut` (+ `throttle.lut`), `aero.ini` + wing `.lut` files, `electronics.ini`, `drs.ini`, `kers.ini` / `ers.ini`, `setup.ini`, `colliders.ini`, `ai.ini`, `fuel_cons.ini`; optional controller files (`ctrl_*.ini`, `steer_brake_controller.ini`). Game files: `system/cfg/assetto_corsa.ini` (`Telemetry::init`, car 0), `system/cfg/tyre_smoke.ini` (`DriftModeComponent::init`) | real files, read through `INIReader` |
| Setup | none needed: `SetupManager::init` @ `0x140289290` reads only `car.ini [RULES] MIN_HEIGHT` and registers the items (`initItems`); it does not open `setup.ini`. Each item's start value is taken from the physics member it points at. `setup.ini` is opened by `BrakeSystem::loadINI` (`[FRONT_BIAS] MIN`, `MAX`) and, for AI cars, by `AIDriver::AIDriver` and `SetupManager::load` @ `0x14028cc90` | — |
| Controls provider | not needed by the constructor (`controlsProvider = null`) | — |
| May stay null | `tag`, `ksPhysics->debugVisualizer`, `ksPhysics->pool`, `rigidAxle` (non-axle cars), all `Car` event handler lists | — |

Hard exits in the constructor: an unknown `[FRONT]/[REAR] TYPE` prints an error and calls
`exit(1)`.

### 9.2 What the first `Car::step` needs on top

| Need | Why | What to do |
|---|---|---|
| A mesh collider on `body` | `Car::updateColliderStatus` → `RigidBodyODE::setMeshCollideMask(0, …)` reads `collisionMeshes[0]` without a bounds check → **null read without one** | after the constructor call `Car::initColliderMesh` with a hand-built `Mesh` (a `vertices` vector of 0x2c-byte records and a `uint16` `indices` vector at the offsets in `re/types/Mesh.txt`), or call `body->addMeshCollider` (+0x120) directly with a small box-shaped mesh. The real `collider.kn5` needs `KN5IO` and a `GraphicsManager`, i.e. the renderer — do not try |
| `controlsProvider` non-null | `Car::step` calls `controlsProvider->vtable[+0x10](4)` **before** testing the pointer *(disassembly, `0x140275f73`)* | a fake object with a 12-slot vtable. **Do not hand a bare fake to `Car::setControllerProvider` @ `0x140275a70`:** it runs `__RTDynamicCast(provider, ICarControlsProvider → AIDriver)` on the new pointer *(disassembly, `0x140275ab8`)*, which reads the RTTI locator stored just before the vtable, and it first deletes any previous provider (slot +0x00 with argument 1). Either write the pointer straight into `Car::controlsProvider` (offset 0x3d80; the function otherwise only fires `evOnControlsProviderChanged`, empty in an oracle) or put the locator pointer of a real provider vtable in front of the fake vtable. Slots called on the step path: +0x08 (`pollControls`: fill `CarControls`), +0x50 (`pollControls`: rpm / limiter feedback), +0x48 (`pollControls`: vibration data), +0x18 (force feedback: `Car::onTyresStepCompleted` every step, and `pollControls` while the controls are locked or the car is black-flagged), +0x10 (`Car::step`: action query, lights); the meaning of the arguments belongs to the steering/FFB map |
| `ksPhysics->physicsTime` advancing | `updateBodyMass` gate, control lock, collision times | set it yourself (in the game `PhysicsEngine::step` copies its second argument) |
| A usable AI spline | `SplineLocator::step` @ `0x1402ab5c0` (in `stepComponents`, every car) calls `locateOnSpline` and `AISpline::payloadAtPosition` with no guard for an empty spline; `Car::postStep` does the same | **probable crash** with an empty `AISplineRecorder`. Give the track a tiny valid `fast_lane.ai` (a few points) or drive `stepComponents`' parts individually. Not verified either way |
| `track->drsMamanger`, `timeLines`, `dynamicGripLevel` | `DRS::step`, `TimeTransponder::step`, tyres | provided by the real `Track::Track` (empty time-line list is fine) |
| Ground under the tyres | tyre rays go through the track | the real `Track::rayCast` on the two-triangle surface, or keep the tyre oracle's analytic plane by faking the `IRayTrackCollisionProvider` vtable (then `updateColliderStatus` needs `surfaceDef` pointers that are valid or null) |
| Collisions | floor boxes vs the flat surface work through real ODE once `core->step` is called and a collision callback is installed (the real `PhysicsEngine` installs itself) | call `core->step(0.003)` after `Car::step`; do not call `PhysicsEngine::step` unless the thread pool is off and `Track::step`/`stepWind` are acceptable |

### 9.3 Recommended oracle recipe (interpretation, not tested)

1. Map `acs.exe`, bind `msvcr120`, `msvcp120`, `kernel32` (plus whatever `INIReaderDocuments`
   needs to find the Documents folder — unknown, see below), write 0 to `INIReader::useCache`.
2. Working directory: a scratch tree with `system/cfg/assetto_corsa.ini` (`[PHYSICS_THREADING]
   THREADS=0`), `system/cfg/tyre_smoke.ini`, `content/cars/<car>/data/*` copied from `cardata/`,
   `content/tracks/flat/` with a minimal `ai/fast_lane.ai`.
3. `PhysicsEngine::PhysicsEngine` → `Track::Track(engine, L"flat", L"")` → `Track::addSurface`
   (flat quad, category 1) → `Track::initAISpline`.
4. `Car::Car(engine, L"<car>", L"")` → `Car::initColliderMesh` (small mesh) → set
   `controlsProvider` (see §9.2 for why not blindly through `Car::setControllerProvider`) →
   `Car::forceRotation` + `Car::forcePosition` to drop the car on the plane.
5. Loop: set `engine->physicsTime += 3.0`; `Car::stepPreCacheValues`; `Car::step(0.003)`;
   `core->step(0.003)`; optionally `Car::postStep`. Record body/hub poses and velocities.

What would probably crash or hang if left out: no mesh collider (certain), null controls provider
(certain), a fake controls provider without RTTI passed to `Car::setControllerProvider` (expected:
`__RTDynamicCast` reads the locator in front of its vtable; not run), empty AI spline (likely),
thread pool running tasks on game code from other threads (avoid with `THREADS=0`), `INIReader` cache (certain unless `useCache = 0`), `data.acd` present
(needs `FolderEncrypter::keys` static initialiser), `Telemetry::init`/`DriftModeComponent::init`
missing config files (unknown — they open `system/cfg` files; behaviour with a missing file was
not checked), `INIReaderDocuments` inside `Track::Track` (needs the shell API that returns the
Documents path; not checked which import it uses).
