# The physics step: full call tree

Source: `acs.exe` + `acs.pdb`, read through the local index only (`re/decomp/` pseudo-C,
`re/types/` layouts, `tools/re_query.py`, `tools/disasm.py` for constants and argument checks).
Folders read: `PhysicsDriveThread`, `PhysicsAvatar`, `PhysicsEngine`, `PhysicsCarStateProvider`,
`ThreadPool`, `TimerProcess`, `Car`, plus every class that subscribes to a step event.
Raw pseudo-C of the key functions cited here: `re/car/car_step/` (140 files, plain copies, plus
`component_callees.txt`). Nothing in the Ghidra project, the game folder or git was changed.
No Rust was written.

Wording used below: **confirmed** = seen in the pseudo-C or disassembly; **interpretation** = my
reading of what the code is for.

Review pass: a second agent re-checked this document against the pseudo-C, the disassembly, the
struct layouts and the ini data (about 55 claims, every address/name pair, all vtable slots and
struct sizes quoted). Corrections and additions are folded into the text; points that could not
be settled are in section 8.
A third, adversarial pass (key `verify_car_step`) re-read the pseudo-C for about 65 more claims
(all formulas and constants of section 5, the ini table, every slot and struct size, the
name/address pairs by script). It found the text accurate apart from the points now corrected in
place: what `valueCache.speed` is stale against (6.3), the `hasGridPosition` condition and the
missing latch of the jump-start test (5.3 step 17), the `forceRotation` / `forcePosition` order
in `addJumpStartPenalty` (5.8), incomplete callee lists in 3.4; plus the replay playback path,
the collision `param2`, the time units and the `rand()` users (open questions, now resolved),
and several load-time reads and helpers that were missing.

---

## 1. Plain-English summary

The game has two main threads. The main thread draws the picture and runs menus, sound and race
rules. A second thread, the physics thread, moves the cars.
The physics thread looks at the clock, and for every 3 milliseconds of real time that have passed
it runs exactly one physics step, so the simulation always advances in equal slices of 0.003 s
(333 per second). If the computer was busy and several slices are owed, it runs them one after
another to catch up; if more than one second is owed it gives up and restarts the clock.
One step does this, in order: run any small jobs the main thread asked for, update track grip and
wind, then for every car read the driver's pedals and wheel (a human device, or the computer
driver, which does its thinking right here), burn fuel, apply penalties, and step every part of
the car (brakes, suspension, tyres, aero, engine and gearbox, driver aids, lap timing).
All of those parts only push forces into the car's rigid bodies. After every car is done, the
rigid-body library works out collisions and moves all bodies forward by 0.003 s.
Then the "step finished" listeners run: each car updates its track position, remote cars are
advanced, and the network client sends the player's position.
Last, still on the physics thread, a snapshot of every car is copied into a shared buffer under a
lock; the replay recorder and the telemetry shared memory are written from that snapshot.
The main thread copies the snapshot out under the same lock once per frame and uses it to place
the car models. When the game is paused the physics thread does nothing but zero the wheel forces
and sleep.
With four or more CPU cores the cars are split over a small pool of worker threads, so several
cars are stepped at the same time.

---

## 2. Classes

| Class | Size | What it is for |
|---|---|---|
| `PhysicsDriveThread` | 0xb8 | The physics thread itself: clock, fixed-step loop, pause flag, time scale, input polling, CPU-load numbers. Member of `PhysicsAvatar`. |
| `TimerProcess` | 0x38 | Alternative to a free-running thread: a Windows multimedia timer (`timeSetEvent`, 3 ms period) that calls `PhysicsDriveThread::step`. Used on machines with fewer than 3 cores. |
| `PhysicsAvatar` | 0x498 | The bridge object (a `GameObject` on the main thread) that owns the `PhysicsEngine`, the drive thread, the command queue main→physics, the mutex and the shared car-state buffer. |
| `PhysicsEngine` | 0x278 | The world: list of cars, track, wind, temperatures, session info, the rigid-body core, the step events, the thread pool, the event queue physics→main. `step` is the body of one physics step. |
| `ThreadPool` | 0x68 | Worker threads + a mutex/condition-variable task queue. Only used to run `Car::step` for some cars in parallel. |
| `Task` | 0x28 | One pool job: `bool isDone` + `std::function<void()>`. |
| `WorkerThread` | 0x30 | Functor each worker runs: `pool`, `initFunction`, `thread_id`. |
| `Car` | 0x3ea0 | One simulated car. Owns every component (see `re/types/Car.txt`). `step` is the per-car part of the physics step. |
| `CarControls` | 0x34 | What the driver asks for this step: gas, brake, steer, clutch, handbrake, gear buttons, DRS/KERS, aid up/down buttons. Filled by the controls provider. |
| `CarControlsInput` | 8 | What the provider is told: `steerLock`, `speed`. |
| `ICarControlsProvider` | 0x18 | Interface "where do the controls come from". Implemented by `DICarControl` (wheel / DirectInput), `KeyboardCarControl` (0x120), `JoypadCarControl` (0x1a0), `AIDriver` (0x3e8), `AIDriverDrag` (0x118). These five are the only vtables with an `acquireControls` in slot +0x08 (confirmed with `re_query.py slot`): there is **no network and no replay controls provider**. Remote cars and replayed cars never run `Car::step`; they only supply a `CarPhysicsState` (see `ICarPhysicsStateProvider`). |
| `VibrationDef` | 0x14 | Wheel/pad rumble levels sent back to the provider: `curbs`, `gforce`, `slips`, `engine`, `abs`. |
| `CarPhysicsState` | 0xb70 | Flat snapshot of one car for the render side, replay and telemetry (matrices, speeds, tyre state, inputs ...). Filled by `Car::getPhysicsState`. |
| `WingState` | 0x44 | Snapshot of one wing, filled by `Car::getWingState`. |
| `ICarPhysicsStateProvider` | 8 | Interface "give me a `CarPhysicsState`". Implemented by `PhysicsCarStateProvider` (0x10, wraps a local `Car`), `NetCarStateProvider` (0x15b8, a remote car: no `Car` object, but its own `IRigidBody *body` created in its constructor through `core->createRigidBody`, IPhysicsCore +0x18), and `ReplayRecorder` (0x110; the provider is its sub-object at +0x58, `ReplayRecorder::getPhysicsState` 0x14015d850 / `getWingState` 0x14015ff00). |
| `ACPhysicsEvent` | 0x48 | One message physics→main (only type 0 = collision is produced: car id, speed, position, normal, collider type). |
| `OnPhysicsStepCompleted` | 0x10 | Argument of `PhysicsAvatar::evOnStepCompleted`: `double pt`, pointer to the state vector. |
| `OnStepCompleteEvent` | 0x10 | Argument of `Car::evOnStepComplete`: `Car *car`, `double physicsTime`. |
| `SessionInfo` | 0x20 | `type`, `startTimeMS`, `timeSecs`, `laps`, `index`. Copied into the engine on the physics thread. |
| `PhysicsCPUTimes` | 0x28 | Timing numbers: `carStep`, core solver / collision times, contact count. |
| `ThermalObject` | 0x20 | Tiny first-order heat model. `Car::water` (water temperature) is the only one stepped from `Car`. |

Other systems' classes that appear only as callees (brakes, suspension, drivetrain, aero,
electronics, AI, replay, network) are listed in section 3 with one line each.

---

## 3. Key functions

### 3.1 Loading / init

```
PhysicsAvatar::PhysicsAvatar                         0x140120930   (main thread)
├─ PhysicsEngine::PhysicsEngine                      0x140262430
│    reads system/cfg/assetto_corsa.ini (section 4), creates the ODE core
│    (ksPhysicsCoreODEFactory::create), core->setCollisionCallback(this) (IPhysicsCore +0x48),
│    PhysicsEngine::initLowSpeedFF 0x140263c90, creates the ThreadPool when allowed
│    └─ ThreadPool::ThreadPool                       0x1402d04a0   starts N WorkerThread threads
│         each worker first runs the init lambda     0x140263010   core->initMultithreading (+0x08),
│                                                                  optional affinity 1<<(id+3), priority 2
├─ PhysicsDriveThread::PhysicsDriveThread            0x140122a60   reads [THREADING] and controls.ini
├─ engine.evOnPreStep            += lambda           0x140121af0   → PhysicsAvatar::stepCommandQueue
├─ driveThread.evPhysicsStepCompleted += lambda      0x140121a50   → PhysicsAvatar::onPhysicsStepCompleted
├─ engine.evOnFlagEvent          += lambda           0x1401214e0   forwards flags to the main thread
└─ sim->evOnNewSession           += lambda           0x140121220   queues setSessionInfo for the physics thread

Car::Car                                             0x14026bf00   (one per local car)
├─ physicsGUID = number of cars already in engine->cars   (first car created = 0)
├─ Car::initCarData 0x140272b30, BrakeSystem::init, suspensions, Tyre::init x4, heave springs,
│  aero, ERS/Kers, steering, drivetrain, autoclutch/blip/shifter, gear changer, EDL, ARBs, ABS, TC,
│  speed limiter, collider, setup, telemetry (GUID 0 only), spline locator, ESC, drift, performance
│  meter, lap invalidator, penalties, transponder, slipstream, fuel evaluator
│  Car's own load helpers called here: Car::initCarDataPath 0x1402739c0 (first), Car::initHeaveSprings
│  0x140273f40, Car::initAeroMap 0x140272a80, Car::buildARBS 0x14026f750, Car::updateBodyMass (once,
│  right after the car is pushed into engine->cars); Car::initCarData → Car::initPitstopTimings
│  0x140274020. Car::initColliderMesh 0x140273b20 is called later, from CarAvatar::initPhysics.
├─ tyres[3].onStepCompleted = lambda                 0x14026eee0   → Car::onTyresStepCompleted
├─ engine->evOnNewSessionPhysics += lambda           0x14026eef0   → Car::onNewSession 0x140274c30
└─ engine->evOnStepCompleted     += lambda           0x14026ef00   → Car::postStep(dt = 0)

CarAvatar::initControls                              0x1400d6f30   picks the provider from controls.ini
└─ Car::setControllerProvider                        0x140275a70   DICarControl / KeyboardCarControl /
                                                                   JoypadCarControl / AIDriver
PhysicsAvatar::startPhysics                          0x1401223a0   (called from Sim::Sim)
└─ PhysicsDriveThread::start                         0x140123480
     useTimerProcess == false → std::thread running PhysicsDriveThread::run (launch pad 0x140123030)
     useTimerProcess == true  → TimerProcess::TimerProcess(3 ms, lambda 0x140123020 → ::step) 0x1402d0230
```

### 3.2 Every physics step (execution order)

```
PhysicsDriveThread::run                              0x140123150   thread body, loops until shuttingDown
(or timer2_callback 0x1402d0380 → vtable slot 0 = TimerProcess::step 0x1402d0360 → PhysicsDriveThread::step 0x140123560)
│  first pass only: srand(ksGetSystemTime()), optional affinity mask 4, core->initMultithreading
├─ if isPaused:  DirectInput::poll, DICommandManager::step, PhysicsEngine::stepPaused 0x140265320, ksSleep(10)
└─ else, while currentTime < now:
   ├─ DirectInput::poll 0x14023f160, DICommandManager::step 0x140081730     (human devices)
   ├─ currentTime += 3.0 ms
   ├─ PhysicsEngine::step(0.003, currentTime, now)   0x140264760
   │  ├─ gameTime = now; physicsTime = currentTime; stepCounter++
   │  ├─ evOnPreStep handlers
   │  │   └─ PhysicsAvatar::stepCommandQueue         0x1401223e0   runs every queued main→physics job
   │  ├─ Track::step                                 0x140278d20   dynamic grip level
   │  │   ├─ call with track->drsMamanger to the shared empty function 0x140017870 (does nothing)
   │  │   └─ AISplineRecorder::step                  0x140296f60   records the driven line (when present)
   │  ├─ PhysicsEngine::stepWind                     0x140265380   slow ±10 % gusting of wind speed
   │  ├─ for each car: Car::stepPreCacheValues       0x1402768c0   cached speed, power-to-mass index
   │  ├─ for each car: Car::step(dt)                 0x140275da0   (see 3.3)  physics thread, or a
   │  │                                                            ThreadPool task → lambda 0x140263080
   │  │   busy-wait until every Task.isDone
   │  ├─ core->step(dt)  = PhysicsCore::step         0x1402cd690   (IPhysicsCore +0x10)
   │  │   ├─ PhysicsCore::collisionStep              0x1402cbf90   ODE collision (dSpaceCollide → nearCallback
   │  │   │   │                                                    0x1402ccc70 → PhysicsCore::onCollision 0x1402ccda0);
   │  │   │   │                                                    per contact, through ICollisionCallback +0x08:
   │  │   │   └─ PhysicsEngine::onCollisionCallBack  0x140264020
   │  │   │       └─ for each car: Car::onCollisionCallBack 0x140274650   damage, eventQueue push,
   │  │   │                                                                Car::evOnCollisionEvent
   │  │   └─ dWorldStep                              0x1403404c0   integrate all bodies
   │  ├─ core->getCoreCPUTimes (+0x90) → physicsCPUTimes
   │  ├─ evOnStepCompleted handlers (argument: &physicsTime), in registration order:
   │  │   ├─ Car::postStep (each car)                0x140275430
   │  │   │   └─ Car::evOnStepComplete handlers: GhostCarRecorder::onStepComplete 0x1401093f0,
   │  │   │                                       comparative telemetry lambda 0x140073820
   │  │   ├─ NetCarStateProvider::step (each remote car)   0x14011bd00
   │  │   ├─ PhysicsObject lambda (each movable track object) 0x1402acb00
   │  │   └─ ACClient::onPhysicsStep (online only)   0x14004f1d0
   │  └─ test mode only: checks, PhysicsEngine::writeTestResult 0x140265530, exit(0)
   ├─ occupancy / cpuTime bookkeeping (atomics)
   └─ evPhysicsStepCompleted handlers
       └─ PhysicsAvatar::onPhysicsStepCompleted      0x1401221a0
           ├─ PhysicsAvatar::updateCrossThreadValues 0x1401228a0   temperature / wind → main thread
           ├─ lock mutex
           │   └─ PhysicsEngine::getPhysicsStates    0x140263ac0
           │       ├─ per car: Car::getPhysicsState 0x140270d70, Car::getWingState 0x140272a20
           │       └─ per extra provider: getPhysicsState / getWingState (virtual +0x08 / +0x10),
           │                              e.g. NetCarStateProvider::getPhysicsState 0x140119220
           ├─ unlock mutex
           ├─ DRSManager::wasRaceSwitchedOn 0x140279510 → message via Sim::executeOnMainThread
           └─ PhysicsAvatar::evOnStepCompleted handlers ({pt, &currentStates}):
               (in registration order: Sim::Sim 0x140192070 builds SharedMemoryWriter, then
                ReplayManager, then FormTelemetry)
               ├─ SharedMemoryWriter::updatePhysics  0x140186ef0   fills the acpmf_physics page
               ├─ ReplayManager::onStepCompleted     0x140158b50   → ReplayRecorder::recordFrame 0x1401609d0
               └─ FormTelemetry lambda               0x1403316e0   dev telemetry app, every 12 ms
   after the loop: "WAS LATE, HAD TO LOOP %d times" when more than one step ran
   run() only: ksSleep(0) between passes
```

### 3.3 `Car::step` (per car, per step)

```
Car::step(dt)                                        0x140275da0
├─ only if physicsGUID == 0: ERP/CFM switch
│    suspension->setERPCFM (ISuspension +0xb8) x4:  Suspension::setERPCFM 0x1402c3230,
│    SuspensionStrut::setERPCFM 0x1402c61b0, SuspensionAxle::setERPCFM 0x1402c8680, (SuspensionML: empty)
│    fuelTankJoint->setERPCFM (IJoint +0x08) = FixedJointODE::setERPCFM 0x1402cd5a0
├─ Car::pollControls(dt)                             0x140274e70
│   ├─ controlsProvider->acquireControls(&controls, dt, &{steerLock, speed})   (+0x08)
│   │    DICarControl::acquireControls 0x14007fe70 | KeyboardCarControl::acquireControls 0x14010f2e0
│   │    JoypadCarControl::acquireControls 0x1401dbce0 | AIDriverDrag::acquireControls 0x1402ac1f0
│   │    AIDriver::acquireControls 0x14029baa0   ← the whole AI driver runs here:
│   │       stepPushVariations 0x1402a1f20, stepSetup 0x1402a2020, stepHumanizer 0x1402a19e0,
│   │       stepSplineSelection 0x1402a2240, stepPitLaneMode 0x1402a1d00, stepSinOffset 0x1402a2170,
│   │       updateCurrentSplinePos 0x1402a3200, stepCarsAwareness2 0x14029f8d0, stepSteer 0x1402a2630,
│   │       stepGears 0x1402a1710, stepGasBrake 0x1402a08e0, stepTurbo 0x1402a30c0, stepDRS 0x1402a0850,
│   │       stepKers 0x1402a1bb0, stepP2P 0x1402a1c20, stepSuperhuman 0x1402a2a60
│   ├─ controlsProvider->setEngineRPM(rpm, limiter*0.75) (+0x50)   DICarControl::setEngineRPM 0x140081000
│   └─ controlsProvider->setVibrations(VibrationDef) (+0x48)       DICarControl::setVibrations 0x140081010
├─ controlsProvider->getAction(4) (+0x10)  → toggles lightsOn on a rising edge
│    (called with NO null check: a car without a provider crashes here, see 5.3 step 4)
├─ if blackFlagged and not Car::isInPits 0x140274530: Car::forceRotation 0x140270040,
│                                                     Car::forcePosition 0x14026fe10 (teleport to pit box;
│                                                     full car reset, see 5.8)
├─ Car::updateAirPressure                            0x140276ae0   air density incl. slipstream → aeroMap.airDensity
│   └─ PhysicsEngine::getAirDensity 0x140263a60, SlipStream::getSlipEffect 0x1402aac60
├─ fuel burn (inline), engine fuelPressure
├─ Car::updateBodyMass                               0x140276c70   once per second of physics time
│   └─ Car::calcBodyMass 0x14026fb70, body->setMassBox (+0x38), fuelTankBody->setMassBox
├─ control overrides: locked controls, gentle stop, penalties (PerformanceMeter::getCurrentSplit 0x14026b700)
├─ finalSteerAngleSignal
├─ Autoclutch::step                                  0x1402b9590   → Autoclutch::stepSequence 0x1402b97a0
├─ sleeping logic: body->stop, fuelTankBody->stop (IRigidBody +0x70 = RigidBodyODE::stop 0x1402cead0)
├─ accG from velocity change
├─ Car::stepThermalObjects                           0x1402769f0
│   └─ ThermalObject::addHeadSource 0x1402b2ef0, ThermalObject::step 0x1402b2f00   (water temperature)
├─ Car::stepComponents                               0x1402764d0   (see 3.4)
├─ Car::updateColliderStatus                         0x140276df0   collision mask of the body mesh
└─ only if physicsGUID == 0: Car::stepJumpStart      0x140276780
    └─ PhysicsEngine::hasSessionStarted 0x140263c70, PenaltyManager::addJumpStartPenalty,
       Car::evOnJumpStartEvent → ACClient lambda 0x14003d170 (tells the server)
```

### 3.4 `Car::stepComponents` — every callee, with its own direct callees

Order is exactly the order in the code. "→" lists the direct callees (from `re_query.py callees`).
Left out of the lists unless they are the only callee: `Car::getSpeed` 0x140272160,
`Car::getControlsProvider` 0x140270650 (called by `Telemetry::step` and `LapInvalidator::step`),
`Drivetrain::getEngineRPM`, the `RaceEngineer` constructor/destructor temporaries
(`AutoShifter::step`, `SpeedLimiter::step`, `DriftModeComponent::step` each build one on the
stack) and CRT/STL helpers.

| # | Callee | Address | Direct callees (one level down) |
|---|---|---|---|
| 1 | `BrakeSystem::step` | 0x14028e640 | `DynamicController::eval` 0x1402b0c00, `BrakeSystem::stepTemps` 0x14028e920, `BrakeSystem::saveTempsRunFame` 0x14028e4e0 |
| 2 | `EDL::step` | 0x1402bb460 | none (body not read here; electronics map) |
| 3 | `suspension->step` (ISuspension +0xb0), 4 times | per class: `Suspension::step` 0x1402c3390, `SuspensionStrut::step` 0x1402c6600, `SuspensionAxle::step` 0x1402c8770, `SuspensionML::step` 0x1402cab00 | `Damper::getForce` 0x1402b3280 in all four; `ActiveActuator::eval` 0x1402cb610 only in `Suspension::step` |
| 4 | `Tyre::step` x4 (tyre index 0..3) | 0x140283800 | see `docs/map/tyre.md`. At the end of tyre 3 only: `onStepCompleted` → `Car::onTyresStepCompleted` 0x140274cd0 → `Car::getSteerFF` 0x140272180, `controlsProvider->sendFF` (+0x18, `DICarControl::sendFF` 0x140080e40) |
| 5 | `HeaveSpring::step` x2 (skipped when `k == 0`) | 0x1402b3960 | `Damper::getForce` |
| 6 | `DRS::step` | 0x1402b4e60 | `DRSManager::isDRSAvailable` 0x140279490, `Wing::setOverrideAngle` 0x1402b2bb0, `Wing::clearOverrides` 0x1402b2b70 |
| 7 | `AeroMap::step` | 0x1402b7150 | `AeroMap::addDrag` 0x1402b5860, `AeroMap::addLift` 0x1402b5a90, `Wing::step` 0x1402b2bc0 (→ `Wing::stepDynamicControllers` 0x1402b2dd0, `Car::getGroundWindVector` 0x140270c00, `RaceEngineer::getPointGroundHeight` 0x14027c550, `atanf`, `Wing::addDrag` 0x1402b2420, `Wing::addLift` 0x1402b2730) |
| 8 | `Kers::step` (only if `kers.present`) | 0x1402b7e10 | `Car::isInPits`, `DynamicController::eval`, `Drivetrain::getEngineRPM` 0x140266b80, `Curve::getValue` |
| 9 | `ERS::step` (only if `ers.present`) | 0x1402930e0 | `Car::isInPits`, `Engine::getMaxTurboBoost` 0x140285af0, `DynamicController::eval`, `Drivetrain::getEngineRPM`, `Curve::getValue` |
| 10 | `SteeringSystem::step` | 0x1402b81b0 | `DynamicController::eval` |
| 11 | `AutoBlip::step` | 0x1402b9ef0 | `Curve::getValue` |
| 12 | `AutoShifter::step` | 0x1402ba7f0 | `PhysicsEngine::hasSessionStarted`, `Engine::getMaxPowerRPM` 0x140285a70, `Engine::getMaxTorqueRPM` 0x140285ae0, `RaceEngineer::getDrivingTyresSlip` 0x14027bc20, `Drivetrain::isChangingGear` 0x1402673b0 |
| 13 | `GearChanger::step` | 0x1402bab50 | `Drivetrain::gearUp` 0x1402669f0, `Drivetrain::gearDown` 0x140266660, `Drivetrain::setCurrentGear` 0x1402692e0 |
| 14 | `Drivetrain::step` | 0x14026b130 | `Drivetrain::stepControllers` 0x14026b200, then one of `Drivetrain::step2WD` 0x1402694e0, `Drivetrain::step4WD` 0x14026a220, `Drivetrain::step4WD_new` 0x14026ad80. `step2WD` and `step4WD` → `Drivetrain::reallignSpeeds` 0x140269100, `Engine::step` 0x1402880e0 (→ `Engine::getThrottleResponseGas` 0x140285b90, `Engine::stepP2P` 0x140288750 which can raise `Car::evOnPush2Pass`, `Engine::stepTurbos` 0x140288900, `PhysicsEngine::getAirDensity` 0x140263a60), `Drivetrain::getInertiaFromWheels` 0x140266c20, `Drivetrain::accelerateDrivetrainBlock` 0x1402664c0, `Tyre::stepRotationMatrix` 0x140284b80 (reads `Car::isSleeping` 0x1402745e0 = `sleepingFrames > framesToSleep`); `step4WD_new` → `step2WD`, then `DynamicController::eval`, `getInertiaFromWheels`, `accelerateDrivetrainBlock`, `Tyre::stepRotationMatrix` |
| 15 | `AntirollBar::step` x2 | 0x1402bb640 | `DynamicController::eval` |
| 16 | `ABS::step` | 0x14028f610 | `Car::getSpeed` only |
| 17 | `TractionControl::step` | 0x140290200 | `Car::getVelocity` 0x140272940 |
| 18 | `SpeedLimiter::step` | 0x1402bb910 | `RaceEngineer::getOptimalBrake` 0x14027c320 |
| 19 | `CarColliderManager::step` | 0x1402a3d50 | `FileChangeObserver::hasChanged` 0x14023b440, `CarColliderManager::loadINI` 0x1402a37a0 (interpretation: reloads the collider file when it changes on disk) |
| 20 | `SetupManager::step` | 0x14028d090 | `SetupManager::isSetupRespectingRules` 0x14028cc10 |
| 21 | `Telemetry::step` (GUID 0 only) | 0x1402bf5b0 | `AeroMap::getCurrentLiftKG` 0x1402b5b90, `AeroMap::getCurrentDragKG` 0x1402b5b60, `DynamicController::getRearSpeedRatio` 0x1402b1230, `DynamicController::getOversteerFactor` 0x1402b11d0, `TyreThermalModel::getAvgSurfaceTemp` 0x1402adb90 |
| 22 | `DriftModeComponent::step` (GUID 0 only) | 0x1402bff20 | `DriftModeComponent::validateDrift` 0x1402c0270, `RaceEngineer::getBetaRAD` 0x14027ba90, `Car::getLocalVelocity` 0x140270d20, `DriftModeComponent::checkExtremeDrifting` 0x1402bfc50 |
| 23 | `PerformanceMeter::step` (GUID 0 only) | 0x14026b8e0 | `Car::getSpeed` only |
| 24 | `LapInvalidator::step` (GUID 0 only) | 0x1402c0580 | `Car::getPenaltyTime` 0x140270d50, `LapInvalidator::onEnterPenaltyZone` 0x1402c03d0 (→ `Car::addPenalty`), `Car::clearPenalty` 0x14026fc50 |
| 25 | `PenaltyManager::step` (GUID 0 only) | 0x140265e10 | `Car::isInPitLane` 0x1402744e0, `Car::setBlackFlag` 0x1402759f0 |
| 26 | `SplineLocator::step` | 0x1402ab5c0 | `SplineLocator::locateOnSplineWithBounds` 0x1402ab400, `SplineLocator::locateOnSpline` 0x1402ab2f0, `InterpolatingSpline::getSignedDistanceFromSpline` 0x1401f2df0, `AISpline::payloadAtPosition` 0x1402a90a0 |
| 27 | `StabilityControl::step` | 0x1402bfa50 | `Car::getSpeed`, `atanf` |
| 28 | `TimeTransponder::step` | 0x1402911f0 | `Tyre::getWorldPosition` 0x140280630, `TimeLine::check` 0x1402caef0, `TimeTransponder::getStatus` 0x1402906e0, `TimeTransponder::onTimeLinePassed` 0x140290c20 (→ `PhysicsEngine::getOddTimeOffset`, `Car::getPenaltyTime`, `Car::setBlackFlag`, `TimeTransponder::invalidate` 0x1402908f0, `PenaltyManager::checkBlackFlag` 0x140265c10, `PenaltyManager::decreasePitPenaltyLaps` 0x140265c30, `Kers::reset`, `ERS::reset`), `TimeTransponder::isValid` 0x140290980, `TimeTransponder::lap` 0x140290a50, `TimeTransponder::split` 0x140291150 — **lap timing happens here**; `lap`/`split` trigger `Car::evOnLapCompleted` / `evOnSectorSplit` |
| 29 | `FuelLapEvaluator::step` | 0x14028d360 | `Car::getSpeed`, `Car::isInPitLane` |

---

## 4. Data it reads

Only the scheduler and the car-level glue read data here; every component reads its own files
(see the other maps).

| File | [SECTION] KEY | Goes to | Default / rule | Notes |
|---|---|---|---|---|
| `system/cfg/assetto_corsa.ini` | `[THREADING] USE_TIMER_PROCESS` | `PhysicsDriveThread::useTimerProcess` | `-1` = automatic: timer process only when the machine has fewer than 3 logical CPUs; else `value != 0`. Only read when the section exists (member starts `true`). | shipped value `-1` |
| same | `[THREADING] SET_THREAD_AFFINITY_MASK` | `PhysicsDriveThread::setAffinityMask`, pool init lambda | `true` only when value ≠ 0 **and** at least 5 logical CPUs | shipped `0`. When on: main thread mask 2, physics thread mask 4, worker `i` mask `1<<(i+3)`, worker priority 2 |
| same | `[PHYSICS_THREADING] THREADS` | thread count of `PhysicsEngine::pool` | `-1` (or section missing) = `logicalCPUs / 2`; `0` = no pool; with fewer than 4 logical CPUs never a pool | shipped `-1`, so the pool is **on** on any modern PC |
| same | `[FF_EXPERIMENTAL] ENABLE_GYRO` | `PhysicsEngine::gyroWheelGain = 0.004` when non-zero, else 0 | shipped `0` | |
| same | `[FF_EXPERIMENTAL] DAMPER_MIN_LEVEL`, `DAMPER_GAIN` | `damperMinValue`, `damperGain` (used in `Car::onTyresStepCompleted`) | shipped `0.0`, `1.0` | |
| same | `[LOW_SPEED_FF] SPEED_KMH`, `MIN_VALUE` | `PhysicsEngine::mzLowSpeedReduction.speedKMH`, `.minValue` (used in `Car::onTyresStepCompleted`) | constructor defaults 3.0 and 0.5, overwritten only when the section exists | shipped `3`, `0.01`. Read by `PhysicsEngine::initLowSpeedFF` 0x140263c90 |
| Documents `cfg/controls.ini` | `[HEADER] INPUT_METHOD` | `PhysicsDriveThread::useDirectInput = false` when the value is `EXTERNAL` | else `true` | also read by `CarAvatar::initControls` to pick the provider class (its strings: `WHEEL`, `X360`, `KEYBOARD`, `AI`) |
| car `car.ini` | `[CONTROLS] STEER_LOCK`, `STEER_RATIO` | `Car::steerLock`, `Car::steerRatio` | always overwritten in `Car::initCarData` (unconditional `getFloat`: a missing key gives 0, the constructor's 200.0 / 12.0 do not survive; with `steerRatio` 0 the division is not finite and the signal is forced to 0) | used for `finalSteerAngleSignal`. `STEER_RATIO` is negative in some cars (e.g. -18.5 in 6), so the sign of the signal flips with the data |
| car `car.ini` | `[CONTROLS] FFMULT`, `STEER_ASSIST` | `Car::ffMult = FFMULT * 0.001`, `Car::steerAssist` | `STEER_ASSIST` 0 or missing → 1.0 | read in `Car::initCarData` next to the keys above; used by `Car::getSteerFF` (called from `onTyresStepCompleted`); the maths belongs to the steering map. Both keys in 113 of 113 cars |
| car `fuel_cons.ini` (opened as `content/cars/<unixName>/data/fuel_cons.ini`, at the end of `Car::Car`) | `[FUEL_EVAL] KM_PER_LITER` | `Car::expectedFuelPerLap = bestLapSpline length / (KM_PER_LITER * 1000)` | only when the section exists **and** the track has a best-lap AI spline; otherwise (value still 0) `RaceEngineer::evaluateFuelPerLapFromTrackSpline()` | in 113 of 113 cars. Not used by the fuel burn; read by `Car::setFuelForLaps` 0x140275b60 (AI pit strategy) when the `FuelLapEvaluator` has no measured lap yet. The same constructor tail sets `carHalfWidth = max(front track, rear track) * 0.5 + 0.3` and a first `powerClassIndex = maxPowerW / Car::getTotalMass(true)` |
| car `car.ini` | `[FUEL] CONSUMPTION`, `MAX_FUEL`, `FUEL` | `Car::fuelConsumptionK`, `Car::maxFuel`, `Car::fuel` (start level, also copied to `requestedFuel`) | `MAX_FUEL` 0 or missing → 30.0; `FUEL` 0 or missing → 30.0 | used by the fuel burn in `Car::step`. The SDK example car comments it as "in one second the consumption is (rpm*gas*CONSUMPTION)/1000 litres", which matches the code. All three keys are in 113 of 113 cars (`FUEL=30` in 99) |
| car `car.ini` | `[BASIC] TOTALMASS`, `INERTIA`, `[EXPLICIT_INERTIA] INERTIA`, `[FUEL_EXT] KG_PER_LITER`, `[FUELTANK] POSITION` | `Car::mass`, `bodyInertia`, `explicitInertia`, `fuelKG`, `fuelTankPos` | If the **section** `[EXPLICIT_INERTIA]` exists, its `INERTIA` goes to `explicitInertia` and `[BASIC] INERTIA` is not read (so `bodyInertia` stays (0,0,0)); else `[BASIC] INERTIA` → `bodyInertia`. The section is in 0 of 113 cars, so the "explicit" branch of `updateBodyMass` is not taken with shipped data. `fuelKG` constructor default 0.74, replaced only when `[FUEL_EXT]` exists (2 of 113 cars, both 0.76) | used by `Car::updateBodyMass`; details belong to the body map |

Not read anywhere in this build (confirmed with `re_query.py uses`): `[DEBUG] PHYSICS_TIME_SCALE`
from the shipped `assetto_corsa.ini`.

Hard-coded numbers: step 0.003 s (3.0 ms), reset threshold 1000 ms, "late" counter only after
30 000 ms of game time, paused sleep 10 ms, `Car::framesToSleep = 50`,
`Car::slipStreamEffectGain = 1.0` (see 5.3 step 6).

Replay record interval `ReplayRecorder::recordIntervalMS` (a static double): set in
`ReplayManager::ReplayManager` 0x140153130 from Documents `cfg/replay.ini` `[QUALITY] LEVEL`:
0 → 120 ms, 1 → 90, 2 → 60, 3 → 30, 4 → 15 (confirmed in its pseudo-C; other values leave the
static's initial value, which was not read). `ReplayManager::load` overwrites it with the
interval stored in a loaded replay file (32.0 when the file's version number is below 13).

Command-line test modes (set in `wWinMain` 0x1400c29c0, three places, confirmed in its pseudo-C):
`-test` → `PhysicsEngine::isTestMode = true`, `testMode = AI`; `-get_car_range` → `CarRange`;
`-get_car_bestlap` → `BestLap`. (A fourth switch `-autodrive` is tested right after; what it sets
was not read.)

---

## 5. Each step: inputs, maths, outputs

### 5.1 Real time → fixed steps (`PhysicsDriveThread::run` / `::step`)

Confirmed from the pseudo-C; both functions contain the same code, `run` adds the outer loop.

```
if isTestMode: timeScale = 20.0
now = ksGetTime() * timeScale                 // ms, double
if now - currentTime > 1000.0: print "RESET PHYSICS TIMER"; currentTime = now   // drop the backlog
loops = 0
while currentTime < now:
    if directInput && useDirectInput: DirectInput::poll(); DICommandManager::step()
    t0 = ksGetQPTTime()
    currentTime += 3.0;  loops++
    PhysicsEngine::step(0.003f, currentTime, now)
    cpuTimeLocal = (float)(ksGetQPTTime() - t0)
    occupancy    = min((int)(cpuTimeLocal * 0.33333334 * 100.0), 300)      // atomic; % of the 3 ms budget
    trigger evPhysicsStepCompleted(engine.physicsTime)
if loops > 0: lastStepTimestamp = now
if loops > 1: print "WAS LATE, HAD TO LOOP %d times"; if now > 30000.0: physicsLateLoops++   // atomic
cpuTimeAtomic = cpuTimeLocal
run() only: ksSleep(0)                        // yield, then test shuttingDown and go again
```

- There is no cap on the number of catch-up steps other than the 1000 ms reset (so at most about
  333 steps in one burst).
- `timeScale` (double, default 1.0) multiplies the clock, not `dt`: at scale 5 the loop simply
  owes five times as many 0.003 s steps. Writers found: test mode (20.0) and a `FormAI` dev-app
  lambda (5.0, file `re/decomp/FormAI/lambda_d813fa__operator_call__1402e05b0.c`).
  `PhysicsAvatar::timeMult` is set to 1.0 in the constructor and no other reference was found.
- Paused branch: poll input, `occupancy = 0`, `cpuTimeLocal = 0`,
  `PhysicsEngine::stepPaused`, `ksSleep(10)`. `stepPaused` takes car 0's controls provider and
  calls `setVibrations(all zero)` (+0x48) and `sendFF(0, 0, 0)` (+0x18). Nothing else moves.
  `currentTime` is not advanced while paused, so after a pause longer than one second the
  1000 ms reset fires on resume instead of a catch-up burst.
- Pause is entered through `PhysicsAvatar::pausePhysics` 0x140122300 (callers:
  `Sim::setPauseMode` 0x14019e6d0, `Sim::onReplayModeChanged` 0x14019e0c0, `ReplayManager::load`,
  and a `PhotoMode` lambda 0x1403233b0). `pausePhysics(true)` sets `PhysicsAvatar::isPaused` and
  `driveThread.isPaused`; `pausePhysics(false)` clears the first and calls
  `PhysicsDriveThread::start` 0x140123480, which, once the thread exists, only clears
  `driveThread.isPaused`.
  It is never paused while an online client exists (`sim->client != null` → pause flag false).
  `Sim::setPauseMode` also calls `CarAvatar::setControlsLock(car 0, paused)` (a command-queue job),
  so online the pause menu locks the player's controls instead of stopping physics. It does
  nothing to physics while a replay file is loaded.
- **Replay mode**: entering replay calls `pausePhysics(sim->client == null)` (confirmed: paused
  offline, **not paused online**) and sleeps 500 ms. Leaving replay un-pauses (only when no replay
  file is loaded).
  Confirmed pieces of the playback path: while `PhysicsAvatar::isPaused` is set,
  `PhysicsAvatar::update` returns at once, so no `CarPhysicsState` reaches the car models from
  physics; `CarAvatar::onStartReplay` 0x1400d9400 saves `CarAvatar::physicsStateProvider` in
  `nonReplayPhysicsStateProvider` and points it at the car's `ReplayRecorder`
  (its `ICarPhysicsStateProvider` sub-object), `CarAvatar::onStopReplay` 0x1400d9520 restores it.
  The per-frame playback itself does **not** go through that pointer (confirmed): on the main
  thread `Sim::update` 0x14019ef90 → `ReplayManager::updatePlayState` 0x14015b9a0 →
  `ReplayManager::updateReplayRecorders` 0x14015bc90, which for every recorder sets `frameBlend` /
  `isRewinding` and calls the recorder's own provider sub-object (recorder + 0x58, virtual +0x08 =
  `ReplayRecorder::getPhysicsState` 0x14015d850, +0x10 = `getWingState` 0x14015ff00) straight
  into `recorder->avatar->physicsState` / `wingsStatus`, then `CarAvatar::makeBodyMatrix`
  0x1400d8ec0. This whole path runs on the main thread.
  The replay is *recorded* on the physics thread (section 3.2, `ReplayManager::onStepCompleted`):
  when `pt - lastRecordTime >= ReplayRecorder::recordIntervalMS` and no replay file is loaded, one
  `ReplayRecorder::recordFrame` per car from the fresh `CarPhysicsState` vector. The same call
  then fills one `frameInformation` entry (two 16-bit values of global data, and per movable
  track object values derived from `PhysicsObject::getWorldMatrix` with `atan2f` / `asinf`);
  that part was only skimmed.

### 5.2 `PhysicsEngine::step(dt, physicsTime, gameTime)` line by line

1. `gameTime = arg3`, `physicsTime = arg2`, `stepCounter++`.
2. Call every `evOnPreStep` handler (only `PhysicsAvatar::stepCommandQueue`: pop and run every
   `std::function` in `PhysicsAvatar::commandQueue`).
3. `Track::step(dt)`: if the dynamic track is on (`dynamicTrack.enabled`),
   `grip = (float)(sum of transponder.lapCount over engine->cars) * gripPerLap + baseGrip`,
   `dynamicGripLevel = clamp(grip, 0.85, 1.0)` (confirmed in the disassembly: above 1 → 1.0;
   below 0, below 0.85 or NaN → 0.85; else the value. The `0.0` store the decompiler shows is
   unreachable). When off and not `isExternal`, `dynamicGripLevel = 1.0`; when off and external the
   value is left alone. Then `AISplineRecorder::step` (only when the recorder exists).
4. `stepWind`: if `wind.speed >= 0.01`:
   `s = wind.speed * (1 + 0.1 * sin((physicsTime - sessionInfo.startTimeMS) * 0.0001))`,
   `wind.vector = normalize(wind.vector) * s` (kept only if all three results are finite).
   Constants confirmed in the disassembly. `dt` is not used.
5. `t0 = ksGetQPTTime()`; for each car `Car::stepPreCacheValues(dt)`:
   `valueCache.speed = |body->getVelocity()|`;
   `powerClassIndex = Engine::getMaxPowerW() / (body->getMass() + sum of the 4 suspension->getMass())`.
6. Step the cars:
   - no pool: `for car in cars: Car::step(car, dt)` in list order.
   - pool: `n = cars.size()`, `T = pool->workers.size()`, `chunk = n / T + 1`.
     Cars `chunk .. n-1` each become one `Task` (`ThreadPool::addTask` 0x1402d0f30: lock, push
     pointer, unlock, signal). Cars `0 .. chunk-1` are stepped on the physics thread right after.
     Then `for task in tasks: while (!task.isDone) {}` — an empty spin loop, no sleep or yield.
     A worker (`WorkerThread::operator()` 0x1402d0a00) waits on the condition variable, pops one
     task, runs it, sets `isDone = true`.
     With one car and `T >= 2`, `chunk = 1` and nothing goes to the pool.
     The division is unsigned 64-bit (`div rcx` at 0x140264939) and there is no guard on
     `n - chunk`: with `T = 1` (`THREADS=1`) `chunk = n + 1`, so `n - chunk` wraps to a huge value
     that is handed to `vector<Task>::resize`. Interpretation, not run: that setting throws / crashes
     on the first step. The automatic setting gives `T = logicalCPUs / 2 >= 2`, which is safe for
     any `n >= 1`. With `n = 0` (no cars yet) `chunk = 1` for every `T`, so the same wrap happens
     whenever a pool exists; whether a step can run before the first car is in the list was not
     checked (open question 14).
     Because `chunk >= 1`, **car 0 is always stepped on the physics thread itself**; so are the
     GUID-0-only parts (ERP/CFM switch, telemetry, drift, performance meter, lap invalidator,
     penalty manager, jump start).
7. `physicsCPUTimes.carStep = ksGetQPTTime() - t0`.
8. `core->step(dt)` (collision + `dWorldStep`), then copy `core->getCoreCPUTimes()`.
9. Call every `evOnStepCompleted` handler with `&physicsTime`. `physicsCPUTimes.currentCPU = 0`.
10. Test mode exits (all on car 0, all end with `writeTestResult` + `exit(0)`):
    - `AI`: any tyre with `speed*3.6 > 20` and `status.slipAngleRAD > 0.785385` → `FAIL|HIGH SLIP|npos`;
      any `damageZoneLevel[i] > 0` → `FAIL|DAMAGE|npos`; four tyres on a surface with
      `isValidTrack == false` → `FAIL|TYRES OUT|npos`; `transponder.lapCount > 1` → writes the best lap.
    - `CarRange`: `lapCount > 3` → writes `unixName | (totalM*0.001/totalLiters) * maxFuel`.
    - `BestLap`: `lapCount > 1` → writes `unixName | bestLap as time | bestLap as number`.

### 5.3 `Car::step(dt)` in full

Reads `controls`, `tyres[i].status.load`, body velocity; writes `controls` (overrides), `fuel`,
`finalSteerAngleSignal`, `accG`, `lastVelocity`, `sleepingFrames`, engine `fuelPressure`.

1. **ERP/CFM switch — only when `physicsGUID == 0`** (confirmed; other cars keep whatever their
   joints were created with). `v² = |body->getVelocity()|²`.
   - `v² >= 1.0`: each `suspension->setERPCFM(0.3, suspension->baseCFM)`; tank joint
     `setERPCFM(0.3, -1.0)`.
   - else: each `suspension->setERPCFM(0.9, 1e-7)`; tank joint `setERPCFM(0.9, -1.0)`.
   (`-1.0` = leave CFM alone, per the physics engine map.)
2. `locked = isControlsLocked || physicsTime < lockControlsTime`.
3. `pollControls(dt)` (5.4).
4. Lights: `a = provider->getAction(4)`; on a rising edge of `a`, `lightsOn = !lightsOn`;
   `lastLigthSwitchState = a`.
   Confirmed in the disassembly (0x140275f73..0x140275f82): the virtual call is made **before**
   the `controlsProvider != null` test, so `Car::step` crashes on a car without a provider even
   though `pollControls`, step 9 and `onTyresStepCompleted` all test for null. `AIDriver` and
   `AIDriverDrag` put a shared "return false" function in slot +0x10.
5. If `blackFlagged` and not in the pits (`Car::isInPits`: body within 3 m of the `pitPosition`
   translation, `dist² <= 9`): every step, `forceRotation(-pitPosition row 3)` and
   `forcePosition(pitPosition translation, true)` (5.8; among other things this resets `fuel` to
   `requestedFuel` every step).
6. `updateAirPressure()`: `rho = PhysicsEngine::getAirDensity()`. If `slipStreamEffectGain > 0`:
   `e = min over other cars' slipstreams of clamp01(1 - getSlipEffect(myPos) * gain)`,
   `rho = (rho - e*rho) * (0.75 / gain) + e*rho`. Stored in `aeroMap.airDensity`.
   `slipStreamEffectGain` is 1.0 in practice: the constructor stores 1.0 and the only other
   writers found (`Car::setSlipStreamEffects` 0x140275d40, no callers, and
   `CarAvatar::setSlipStreamEffects` 0x1400da740, called from `DrivingAssistManager`) also store
   the constant 1.0 and ignore their first argument. So the branch is always taken and the
   formula is `rho * (0.75 + 0.25 * e)`; with no car ahead (`e = 1`) it is `rho`.
7. **Fuel burn**:
   `fuel -= (double)(|rpm| * dt * acEngine.gasUsage) * (max(turboBoost, 0) + 1.0) * fuelConsumptionK * 0.001 * (double)ksPhysics->fuelConsumptionRate`
   (`rpm = Drivetrain::getEngineRPM()`; the first product is single precision, the rest double).
   `fuel > 0` → `acEngine.fuelPressure = 1`; else `fuel = 0`, `fuelPressure = 0`.
8. `updateBodyMass()`: only when `physicsTime - lastBodyMassUpdateTime > 1000` ms.
   `bodyInertia == (0,0,0)` → `body->setMassExplicitInertia(mass, explicitInertia)` (and prints
   "explicit"); else `body->setMassBox(calcBodyMass(), bodyInertia.xyz)` with
   `calcBodyMass = mass - sum(suspension->getMass()) + ballastKG`.
   `fuelTankBody->setMassBox(max(fuelKG * fuel, 0.1), 0.5, 0.5, 0.5)`.
9. If a provider exists:
   - `locked` → `gas = 0, brake = 1, steer = 0, clutch = 0`.
   - `isGentleStopping` → `gas = 0, brake = 0.2`. The flag is set by a command-queue job
     (lambda 0x1400d0960) pushed by `CarAvatar::setGentleStop` 0x1400da280; its callers are `ACClient` (a lambda, `onNewSession` 0x14004e4a0, `update` 0x140053b30), `DragMode` (`onFinishLanePassed`, `DragStatus::reset`, `update`), `DriftMode` (`modeGameOver` 0x14008b750, `renderHUD`), `TimeAttack` (`update` 0x1401be0b0, `updateGameOver`), `TimeLimitedTest::update` 0x1401bf1c0 and `UDPCommandListener::update` 0x1401d1570.
   - `penaltyTime > 0`, by `ksPhysics->penaltyMode`:
     - `CutGas`: `gas >= 0.1` → `penaltyTimeAccumulator = penaltyTime`; else
       `penaltyTimeAccumulator -= dt`, and at `<= 0`: `penaltyTime = 0`, accumulator 0,
       `disableMinSpeedPenaltyClear = false`. Also, unless `disableMinSpeedPenaltyClear`, body speed
       below 9.722222 m/s (35 km/h) clears the penalty.
     - `RecoverTime`: `penaltyTime = penaltyPerfTarget - performanceMeter.getCurrentSplit().t`.
10. **Final steer angle**: `finalSteerAngleSignal = steerLock * controls.steer / steerRatio`;
    not finite → 0.
11. `allLoaded = every tyre has status.load > 0`. `Autoclutch::step(dt)`.
12. **Sleeping**: with `speed = valueCache.speed`:
    - `speed >= 0.5` or `|body->getAngularVelocity()|² >= 1` → `sleepingFrames = 0`.
    - else: `(gas > 0.01 && clutch > 0.01 && drivetrain.currentGear != 1)` or `!allLoaded` →
      `sleepingFrames = 0`; otherwise `sleepingFrames++`. When `sleepingFrames > framesToSleep`
      (50): `body->stop(0.99)` and `fuelTankBody->stop(0.99)`. `RigidBodyODE::stop` ignores the
      argument and zeroes linear velocity, angular velocity, force and torque.
13. **accG**: `a = (v - lastVelocity) * (1/dt) * 0.101978384` per component (1/g),
    `lastVelocity = v`, `accG = body->worldToLocalNormal(a)`.
14. `stepThermalObjects(dt)`: `rpm > acEngine.data.minimum * 0.8` →
    `water.addHeadSource((rpm / limiterRPM) * 20 * gas + 85)`; then
    `water.step(dt, ambientTemperature, speed)`:
    `t += ((1 - coolSpeedK*speed) * ambient - t) * (1/tmass) * dt * coolFactor`; if heat was added,
    `t += (heat - t) * (1/tmass) * dt * heatFactor`; accumulator reset.
15. `stepComponents(dt)` (3.4).
16. `updateColliderStatus(dt)`: `isCollisionOffForPits = true` while any tyre is on a surface with
    `isPitlane`; once off the pit lane it stays true until no other car body is within 6 m
    (`dist² < 36`). Mask for the body mesh: `2` when `unixName` is `"spectator"`, else `0x1a`
    with pit collisions off or `0x1e` with them on; bit 0 is added when the body's up axis
    `M22 < 0.25` (car on its side or roof). `body->setMeshCollideMask(0, mask)` (+0x130).
17. `physicsGUID == 0` → `stepJumpStart(dt)`: before the session start
    (`!hasSessionStarted(0)`), when `lockGearboxAtStartTimeMS != 0` and `lockControlsTime <= 0`:
    - if `hasGridPosition` **and** `physicsTime >= startTimeMS - lockGearboxAtStartTimeMS`:
      moving more than 0.1 m in the horizontal plane (`dx² + dz² > 0.010000001`) calls
      `PenaltyManager::addJumpStartPenalty` and triggers `evOnJumpStartEvent(physicsTime)`;
    - otherwise store the body position in `gridPosition` and set `hasGridPosition = true`.
      So the reference position is refreshed every step until the lock window opens, and also on
      the first step after anything cleared `hasGridPosition` (`Car::reset`, i.e. every
      `forcePosition`) even inside the window.
    `stepJumpStart` has no latch of its own. What happens next depends on
    `PenaltyManager::addJumpStartPenalty` 0x140265a90 (confirmed in its pseudo-C), by
    `ksPhysics->penaltyRules.jumpStartPenaltyMode`:
    - `eTeleportToPitMode`: `forcePosition(pit position, true)`, then `forceRotation`, then
      `lockControlsUntil(20000.0, sessionInfo.startTimeMS)`. `lockControlsTime > 0` then keeps
      `stepJumpStart` from testing again.
    - `eDriveThroughMode`: sets `pendingPenaltyType = eDriveThrough`, `pitPenaltyLaps`, pushes a
      `PenaltyRecord`, raises `PenaltyManager::evOnPenalty`. Nothing is reset, so while the car
      stays more than 0.1 m away and the session has not started this repeats **every step**
      (one more record, one more `evOnJumpStartEvent`, i.e. one more packet online).
      Interpretation of the confirmed logic; not run.
    - any other mode: nothing, but `evOnJumpStartEvent` is still raised every step.

### 5.4 `Car::pollControls(dt)`

`in = {steerLock, speed = valueCache.speed}`.

- `isControlsLocked || blackFlagged`: zero gas, brake, steer, clutch, `gearUp`, `gearDn`, `kers`;
  `setVibrations(0)`, `sendFF(0, 0, 0)`, `setEngineRPM(0, 1000)`.
- else:
  1. `provider->acquireControls(&controls, dt, &in)`.
  2. `provider->setEngineRPM(Drivetrain::getEngineRPM(), limiterRPM * 0.75)` (`limiterRPM` =
     `Engine::getLimiterRPM` 0x140285a50, engine virtual +0x08).
  3. Build a `VibrationDef` (all terms then multiplied by `clamp01(speed)` where noted):
     - `vibrationPhase += speed * dt`; `slipVibrationPhase += dt`.
     - `curbs`: over the four tyres' `surfaceDef` (null ones skipped): `G = max(0, max(vibrationGain))`,
       `L = average of vibrationLength over the tyres whose surface has both vibrationLength != 0
       and vibrationGain != 0`. If both non-zero:
       `ksSawToothWave(vibrationPhase, L) * clamp01(tyres[0].load) * clamp01(tyres[1].load) * G * clamp01(speed)`,
       then once more `* clamp01(speed)`. (If `G != 0` but no tyre qualifies for the mean, `L` is 0/0 = NaN. The test at 0x14027512f is `ucomiss` + `je` with no parity check, which treats NaN as equal to 0, so the kerb term is skipped and stays 0.)
     - `gforce = sinf(vibrationPhase * 30) * clamp01(|accG.y|)`.
     - `slips`: `m = max(0, max over tyres of status.ndSlip * 0.75)`; `m <= 1` → `m = m²`;
       `sinf(slipVibrationPhase * 120) * clamp01(m * 0.4) * clamp01(speed)`.
     - `engine = clamp01(engine.velocity * 0.15915507 * 60 / limiterRPM)` (rpm / limiter).
     - `abs`: if ABS present and `ABS::isInAction()`:
       `ksSquareWave((float)physicsTime, 100) * clamp01(speed)`, else 0.
  4. `provider->setVibrations(def)`.

  The two wave helpers (confirmed in their pseudo-C):
  `ksSawToothWave(x, L)` 0x14022d2a0 `= ((x - (float)(int)(x / L) * L) - L * 0.5) / (L * 0.5)`
  (a ramp from -1 to +1 every `L` metres of travelled distance, since `vibrationPhase` integrates
  speed); `ksSquareWave(t, p)` 0x14022d2d0 `= sinf(t / p) > 0 ? 1.0 : -1.0`, so the ABS rumble
  flips sign every `π * 100` ms of physics time (about 314 ms), and its argument is
  `physicsTime` cast to `float`.

Steering force feedback is **not** sent here. It is sent from `Car::onTyresStepCompleted`, in the
middle of `stepComponents`, right after the fourth tyre (section 5.5).

### 5.5 `Car::onTyresStepCompleted`

Runs once per step, from the end of `Tyre::step` of tyre index 3 (the callback is attached to
`tyres[3]` only).

```
if no provider: mzCurrent = 0; return
lastFF = getSteerFF()
if mzLowSpeedReduction.speedKMH != 0:
    k = clamp01(speed*3.6 / speedKMH);  lastFF *= (1 - minValue) * k + minValue
if !isControlsLocked && !blackFlagged:
    d = clamp01(1 - speed*3.6*0.1)                       // damper fades out by 10 km/h
    provider->sendFF(lastFF, ((1 - damperMinValue)*d + damperMinValue) * damperGain, userFFGain)
```

### 5.6 `Car::postStep` (after the rigid-body step)

Runs from `PhysicsEngine::evOnStepCompleted`. The lambda passes **`dt = 0.0`** (confirmed in the
disassembly: `xorps xmm1, xmm1` then jump to `Car::postStep`).

1. Trigger `Car::evOnStepComplete({this, physicsTime})`.
2. `slipStream.setPosition(body position, body velocity)`.
3. Copy the spline locator results into `splineLocatorData`: `isOutsideTrackLimits`,
   `npos = normalizedPos`, `lateralOffset = -offset`, `currentIndex`, `splineLength`,
   `sides[0..1]` from `SplineLocator::getSides`, `sidesFromIL` from the best-lap spline payload.
   `sideVelocity = (side0 - previous side0) / dt` — with `dt = 0` this is ±infinity or NaN.

### 5.7 Collisions (`Car::onCollisionCallBack`)

Called by the core for every contact, for every car, from inside `PhysicsCore::collisionStep` on
the physics thread (before `dWorldStep`). `PhysicsEngine::onCollisionCallBack` first swaps the two
bodies so that a car body comes first. For a car whose `body` is one of the two:

- `lastCollisionTime = physicsTime`; relative speed along the normal
  `relSpeed = -dot(vPointMine - vPointOther, normal) * 3.6` (km/h).
- Each side of the contact carries a user object; its virtual +0x18 returns a type number. Types 1
  and 16 never cause damage ("soft" is my label; the enum was not read).
  Car against car (both bodies non-null, neither soft): `lastCollisionWithCarTime = physicsTime`.
- `relSpeed > 0` and not soft: `d = relSpeed * mechanicalDamageRate`; `d > 150` →
  `Engine::blowUp`. The local contact point picks the zone: mostly front/back
  (`|z| / sqrt(x²+z²) > 0.707`) → `damageZoneLevel[0]` (z > 0) or `[1]`; else side → `[2]`
  (x >= 0) or `[3]`; `[4]` always. Each keeps its maximum.
- Zones 0+2, 0+3, 1+2, 1+3 both `> 0` → `suspension[0..3]->setDamage((zoneA + zoneB) * 0.5)`
  (+0x78; argument confirmed in the disassembly). This test is outside the "damaging" branch, so
  it is repeated on every later contact of the body, damaging or not.
- Push one `ACPhysicsEvent` into `PhysicsEngine::eventQueue` for every contact of this car's body
  (also non-damaging ones): `type = 0`, `param1 = (float)physicsGUID`, `param2` = the last float
  argument of the callback = the ODE contact's penetration `depth` (confirmed in
  `PhysicsCore::onCollision` 0x1402ccda0: it passes `dBodyGetData(body1)`, `dGeomGetData(geom1)`,
  `dBodyGetData(body2)`, `dGeomGetData(geom2)`, `&contact.normal`, `&contact.pos`,
  `contact.depth`), `param3 = -1.0`,
  `param4 = relSpeed`, `vParam1` = contact point, `vParam2` = normal, `ulParam0` = type number of
  the second user object (the non-car side; for the second car of a car-to-car contact it is that
  car's own side). That virtual call has no null check. If damaging, also trigger
  `Car::evOnCollisionEvent`.

### 5.8 Small car-level helpers the step depends on

All confirmed from the pseudo-C; one line each.

- `Car::forcePosition(pos, invalidateLap)` 0x14026fe10 (callers: `Car::step` black-flag branch,
  `PenaltyManager::addJumpStartPenalty`, `AIDriver::acquireControls`,
  `AIDriver::stepSplineSelection`, and a `CarAvatar` command-queue job 0x1400d0440; each calls
  `Car::forceRotation` 0x140270040 first, except `addJumpStartPenalty`, which calls
  `forcePosition` first and `forceRotation` after it): `pos.y +=
  RaceEngineer::getBaseCarHeight() + 0.01`; `Car::reset()`; `body->stop`, `body->setPosition`
  (+0x90); fuel tank body moved to `body->localToWorld(fuelTankPos)`; every suspension `stop()`
  (+0x98) and `attach()` (+0x40); `Drivetrain::reset`, `BrakeSystem::reset`, `Tyre::reset` x4,
  `Drivetrain::setCurrentGear(1, true)`; if `invalidateLap`: `TimeTransponder::invalidate`; raise
  `Car::evOnForcedPositionCompleted(pos)`; stop body and tank again; `framesToSleep = 50`;
  `SplineLocator::reset`.
- `Car::forceRotation(heading)` 0x140270040: builds a matrix with rows
  `normalize(-heading.z, 0, heading.x)`, `(0, 1, 0)`, `-heading`, calls `body->setRotation` (+0x98)
  and `fuelTankBody->setRotation` with it, `attach()` (+0x40) on every suspension, then
  `body->stop` and `fuelTankBody->stop`. It does not call `Car::reset`.
- `Car::reset()` 0x1402758e0 (only caller: `forcePosition`): `hasGridPosition = false`,
  `water.t = 60`, `framesToSleep = 50`, `penaltyTime = penaltyTimeAccumulator = 0`,
  `fuel = requestedFuel`, `Kers::reset` / `ERS::reset` when present, `isCollisionOffForPits = false`.
- `Car::isInPits()` 0x140274530: body position within 3 m of the `pitPosition` translation.
- `Car::lockControls(bool)` 0x1402745f0 sets `isControlsLocked` (two `PitStop` lambdas and a
  `CarAvatar` command-queue job).
  `Car::lockControlsUntil(t, now)` 0x140274600: `t == 0` → `lockControlsTime = 0`; a lock still
  running (`physicsTime < lockControlsTime`) → `lockControlsTime += t`; else
  `lockControlsTime = t + now`. Callers: `PenaltyManager::addJumpStartPenalty`, a `CarAvatar`
  lambda, an `ACClient` lambda.
- `Car::addPenalty(t)` 0x14026f6a0 (caller: `LapInvalidator::onEnterPenaltyZone`): if a penalty is
  already running, `disableMinSpeedPenaltyClear = true`; then `penaltyTime += t`,
  `penaltyTimeAccumulator += t`. `Car::getPenaltyTime` 0x140270d50 returns the accumulator while
  `penaltyTime > 0`, else 0.
- `Car::isSleeping()` 0x1402745e0 = `sleepingFrames > framesToSleep`; read by
  `Tyre::stepRotationMatrix`.
- `Car::setBlackFlag(flag, description)` 0x1402759f0 (callers: `PenaltyManager::step`,
  `TimeTransponder::onTimeLinePassed`, a `CarAvatar` command-queue job): sets `blackFlagged` and
  raises `PhysicsEngine::evOnFlagEvent({car, !flag, description})`. `blackFlagged` is what
  switches `pollControls` to its "locked" branch and starts the teleport of 5.3 step 5.
- `Car::onNewSession(info)` 0x140274c30 (handler of `evOnNewSessionPhysics`):
  `isRetired = false`; if `blackFlagged`, clears it and raises `evOnFlagEvent`; then
  `PerformanceMeter::reset`.
- Other writers of state the step reads, reached from command-queue jobs, the network client or
  the AI driver (one line each, bodies confirmed): `Car::setRequestedFuel(v, alsoRequested)`
  0x140275cb0 sets `fuel` (and `requestedFuel`); `Car::setFuelForLaps(laps, mult)` 0x140275b60
  (AI pit strategy) takes the fuel per lap from `FuelLapEvaluator::getFuelPerLap`, or from
  `expectedFuelPerLap` when that is `<= 0`, and stores `laps * perLap * mult` in `fuel` and
  `requestedFuel` only when it is below `maxFuel` (otherwise it changes nothing and returns
  `maxFuel / perLap` laps); `Car::setBallastKG` 0x1402759e0 sets `ballastKG`
  (picked up by the next once-per-second `updateBodyMass`); `Car::setDamageLevel(v)`
  0x140275b20 / `(v, zone)` 0x140275b50 write `damageZoneLevel`;
  `Car::resetSuspensionDamageLevel` 0x140275970 calls `resetDamage()` (+0x80) on every
  suspension; `Car::setRestrictor(v)` 0x140275d10 stores `clamp01(v * 0.0025)` in
  `acEngine.restrictor`.
- `PhysicsEngine::hasSessionStarted(offsetMS)` 0x140263c70 = `physicsTime > sessionInfo.startTimeMS + offsetMS`.
- `PhysicsEngine::getAirDensity()` 0x140263a60 = `1.2922 - ambientTemperature * 0.0041`.
- `PhysicsEngine::getOddTimeOffset()` 0x140263a90 = `(double)(stepCounter % 3)`; used by
  `TimeTransponder::onTimeLinePassed` (the lap-time maths belongs to the timing map).
- `PhysicsEngine::removeCar(car)` 0x140264240 (from `Car::~Car`): erases the pointer from `cars`;
  the remaining cars keep their `physicsGUID`, so GUID and list index can differ afterwards.
- `PhysicsEngine::addAdditionalPhysicsProvider` 0x140263a00 (from `NetCarStateProvider`'s
  constructor) appends to `additionalPhysicsProviders`, the list `getPhysicsStates` walks after
  the local cars. `PhysicsEngine::setWind` 0x1402645a0 and `setSessionStartTimeMS` 0x140264590
  are called from `ACClient` / `RaceManager` code (bodies not read here).
- `PhysicsAvatar::shutdown` 0x140122360 → `PhysicsDriveThread::stop` 0x140123820: sets
  `shuttingDown`, then joins the thread, or deletes the `TimerProcess` and sleeps 500 ms.
  `shutdown` then calls `DirectInput::cleanup` when `useDirectInput`.
  `PhysicsDriveThread::pause` 0x140123140 (`isPaused = true`) has no caller; pausing goes through
  `PhysicsAvatar::pausePhysics` only.
- `PhysicsCarStateProvider::getPhysicsState` 0x140122a40 / `getWingState` 0x140122a50 only forward
  to `Car::getPhysicsState` / `Car::getWingState`. One is created per local car in
  `CarAvatar::CarAvatar` 0x1400cd1a0; the per-step snapshot does not go through it
  (`getPhysicsStates` calls `Car::getPhysicsState` directly).

---

## 6. Connections to other systems

### 6.1 Who subscribes to what

`PhysicsEngine` events (all run on the physics thread):

| Event | Handler | Address | What it does |
|---|---|---|---|
| `evOnPreStep` | `PhysicsAvatar::stepCommandQueue` (via lambda 0x140121af0) | 0x1401223e0 | Runs every job queued with `PhysicsAvatar::executeOnPhysicsThread` 0x140121f20 (pit stop changes, setup, ERS/ABS/engine-brake cycling from `CarAvatar`, session info ...). |
| `evOnStepCompleted` | `Car::postStep` (lambda 0x14026ef00), one per car | 0x140275430 | Section 5.6. |
| `evOnStepCompleted` | `NetCarStateProvider::step` (lambda 0x1401186d0), one per remote car | 0x14011bd00 | Moves a remote car: lag smoothing (`stepLagging` 0x14011cf60, `getSmoothBodyMatrix` 0x140119310), body matrix, spline position, wings (`stepWings` 0x14011d7c0), slipstream (`SlipStream::setPosition`). Remote cars are not `Car` objects; each provider owns its own rigid body (`NetCarStateProvider::body`; interpretation: this is what the local car collides with). |
| `evOnStepCompleted` | `PhysicsObject` lambda, one per movable track object | 0x1402acb00 | Pushes the object's world matrix to a queue and sets its collision mask (0x1f enabled, 0x0c disabled). |
| `evOnStepCompleted` | `ACClient::onPhysicsStep` (lambda 0x14003fc20), online only | 0x14004f1d0 | `ACClient::sendCarPosition` 0x140052230, receives UDP/TCP, `ACClient::updateQOS` 0x1400541d0. |
| `evOnNewSessionPhysics` | `Car::onNewSession` (lambda 0x14026eef0); `Track::initDynamicTrack`, `FuelLapEvaluator::init`, `AIDriver::AIDriver` also call `Event<SessionInfo>::addHandler` | 0x140274c30 | Raised on the physics thread by lambda 0x1401213f0 after `PhysicsEngine::setSessionInfo` 0x140264560; that lambda also calls `core->setNoCollisionSteps(250)`. |
| `evOnFlagEvent` (raised by `Car::setBlackFlag` 0x1402759f0 and `Car::onNewSession`) | `PhysicsAvatar` lambda | 0x1401214e0 | Re-raises the flag on the main thread (`Sim::executeOnMainThread` 0x140199240 → lambda 0x1401211b0 → `PhysicsAvatar::evOnFlagEvent`, heard by `FlagManager`). |

`PhysicsDriveThread::evPhysicsStepCompleted` → `PhysicsAvatar::onPhysicsStepCompleted`
0x1401221a0 (lambda 0x140121a50). It raises `PhysicsAvatar::evOnStepCompleted`, also on the physics
thread:

| Handler | Address | What it does |
|---|---|---|
| `SharedMemoryWriter::updatePhysics` (lambda 0x140185f20) | 0x140186ef0 | Writes the `acpmf_physics` shared-memory page every physics step. The graphics/static pages are written by `SharedMemoryWriter::update` 0x140186230 on the main thread (by its vtable slot; content not read here). |
| `ReplayManager::onStepCompleted` (lambda 0x140154e00) | 0x140158b50 | Replay recording, section 5.1. |
| `FormTelemetry` lambda | 0x1403316e0 | Dev telemetry app: `FormTelemetry::getValuesFromPhysics` 0x140332ef0 when 12 ms have passed. |

Per-car events. Thread: `evOnStepComplete` (from `postStep`), `evOnCollisionEvent` (from the
collision step) and `evOnJumpStartEvent` (car 0 only) are always raised on the physics thread;
`evOnLapCompleted`, `evOnSectorSplit`, `evOnPush2Pass` and `evOnForcedPositionCompleted` are raised
inside `Car::step`, so on the physics thread or a pool worker depending on the car's chunk:

| Event | Raised from | Subscribers found |
|---|---|---|
| `Car::evOnStepComplete` | `Car::postStep` | `GhostCarRecorder::onStepComplete` 0x1401093f0 (lambda 0x140107a00): stores a ghost frame. `ComparativeTelemetryRecorder` lambda 0x140073820 (via 0x1400739f0): pushes npos, speed, gas, brake, gear to a queue. |
| `Car::evOnCollisionEvent` | `Car::onCollisionCallBack` | `AIDriver::AIDriver` 0x140298c60, `ACClient` lambda 0x14003d020 (registered when a car is loaded). |
| `Car::evOnJumpStartEvent` | `Car::stepJumpStart` | `ACClient` lambda 0x14003d170: sends a 2-byte packet to the server. |
| `Car::evOnLapCompleted` | `TimeTransponder::lap` | 16 functions register a handler of this event type, among them `RaceManager::loadSessions`, `ReplayManager::ReplayManager`, `GhostCarRecorder`, `CarAvatar::initPhysics`, `AIDriver`, `ERS::init`, `Kers::init`, `FuelLapEvaluator::init`, `AISplineRecorder::step`, `HotlapMode`, `DragMode`. |
| `Car::evOnSectorSplit` | `TimeTransponder::split` | `CarAvatar`, `RaceTimingServices`, `ACClient`. |
| `Car::evOnForcedPositionCompleted` | `Car::forcePosition` 0x14026fe10 (confirmed, argument = the corrected position; section 5.8). Also reached from main-thread requests, which run as command-queue jobs on the physics thread | `AIDriver::AIDriver`, `AIDriverDrag::AIDriverDrag` 0x1402ab740 (the only users of `Event<vec3f>::addHandler`). |
| `Car::evOnPush2Pass` | `Engine::stepP2P` 0x140288750, i.e. **inside the per-step path** (`Drivetrain::step` → `Engine::step`); also a `PushToPassManager` lambda 0x1400ac0d0 and `ACClient::onMessageTCP` 0x14004c840 (remote activation) | `ACClient::beginUpdateMode` 0x1400446e0. |
| `Car::evOnTyreCompoundChanged` | `Tyre::setCompound` 0x1402834e0 (not per step: setup / pit stop) | `ACClient` lambda 0x14003d020, `CarAvatar::initPhysics` 0x1400d7660. |
| `Car::evOnControlsProviderChanged` | `Car::setControllerProvider` 0x140275a70 | none found by member name. |

No subscriber for Python, audio or the race manager was found on the two `PhysicsEngine` step
events. They are fed differently: audio gets collisions through `eventQueue` on the main thread;
the race manager listens to `evOnLapCompleted`; the Python plugin is updated on the main thread
(`ACPlugin::update` 0x1400bfe00, not read).

### 6.2 Threads and data crossing

| Direction | Mechanism | Details |
|---|---|---|
| main → physics | `PhysicsAvatar::commandQueue` (`concurrent_queue<std::function<void()>>`) | `executeOnPhysicsThread` pushes; drained at the start of every step (`evOnPreStep`). Not drained while paused. |
| physics → main, car state | `PhysicsAvatar::mutex` (critical section) around `currentStates` / `wingStates` / `dynamicTrackStatusPT` | Physics thread: `getPhysicsStates` rewrites every `CarPhysicsState` (0xb70 bytes each) in place, every step. Main thread: `PhysicsAvatar::update` 0x140122480 copies both vectors under the lock once per frame, then per car `CarAvatar::setNewPhysicsState` 0x1400da420 (only when the number of states equals `sim->cars.size()`); that function copies the whole state into `CarAvatar::physicsState`, after blending `steer`, `gas` and `brake` towards the new values with `clamp01(dt * 10)` for every car whose avatar `guid != 0` (display smoothing only). The whole function is skipped while `PhysicsAvatar::isPaused` is set. Both sides take the lock with `TryEnterCriticalSection` then `EnterCriticalSection`, so the physics thread can block for the length of the main thread's copy. One buffer plus a lock; no double buffering. |
| physics → main, events | `PhysicsEngine::eventQueue` (`concurrent_queue<ACPhysicsEvent>`) | Pushed by `Car::onCollisionCallBack`; popped by `Sim::stepPhysicsEvent` 0x14019ebd0 (from `Sim::update` 0x14019ef90): raises `Sim::evOnCollisionEvent` and calls `CarAudioFMOD::onCarHit`. |
| physics → main, jobs | `Sim::executeOnMainThread` (`Sim::chFunctions` queue) | Used by `updateCrossThreadValues` (temperature, wind), flags, DRS message, and by lap-event handlers. |
| physics → main, counters | atomics in `PhysicsDriveThread` | `occupancy`, `cpuTimeAtomic`, `physicsLateLoops`, copied in `PhysicsAvatar::update`. |
| physics → other processes | `SharedMemoryWriter::updatePhysics` | Written on the physics thread; no mutex or critical-section call appears in that function. |

Matrix smoothing on the main thread: `PhysicsAvatar::update` can extrapolate the body, suspension
and tyre matrices by `(game->gameTime.now - state.timeStamp) * 0.001` s (when its absolute value
is below 0.01 s) using `state.velocity` and `state.angularVelocity`, with
`getTimeCorrectedMatrix` 0x140121fd0, but only if `useMatrixSmoothing && timeScale <= 1`.
(The fields are raw offsets in the pseudo-C, resolved against the layouts: `Game` +0x18 =
`gameTime.now`, `CarPhysicsState` +0x788 = `timeStamp`, which `Car::getPhysicsState` fills with
`physicsTime`, +0x274 = `velocity`, +0x298 = `angularVelocity`.) The constructor sets
`useMatrixSmoothing = true` and then `false` at its end, and no other writer was found, so this
code is dead in this build.

`PhysicsAvatar::render` 0x140122320 (main thread): when paused with DirectInput it calls
`DirectInput::forceFF(0)`.

### 6.3 Inputs and outputs of the car-level glue

| Variable | Written by | Read by |
|---|---|---|
| `Car::controls` | `acquireControls` in `pollControls`, then the overrides in `Car::step` | every component in `stepComponents` (brakes #1, steering #10, gear changer #13, drivetrain #14 ...) |
| `Car::finalSteerAngleSignal` | `Car::step` | `SteeringSystem::step` (#10): rod offset `−(signal · linearRatio)` for wheels 0 and 1 (`docs/map/steering.md` 5.2); `DynamicController::getInput` (`WHEEL_STEER_DEG`) |
| `Car::fuel`, `acEngine.fuelPressure` | `Car::step` | `updateBodyMass` (confirmed); the engine (inside #14) and `FuelLapEvaluator::step` (#29) by name |
| `aeroMap.airDensity` | `Car::updateAirPressure` | `AeroMap::step` (#7) |
| `Car::accG` | `Car::step` | vibration in `pollControls` (next step), telemetry, `CarPhysicsState` |
| `valueCache.speed`, `powerClassIndex` | `Car::stepPreCacheValues` | `Car::getSpeed` 0x140272160 everywhere. Inside `Car::step` the cache equals the live body speed (velocities only change in `core->step`, or through `body->stop` / `forcePosition`, after which the cache keeps the old value until the next step). It **is** one step old for everything that runs after `core->step`: the `evOnStepCompleted` handlers, `Car::getPhysicsState` (the `speed` of the snapshot is the pre-integration speed while its matrices and `velocity` are post-integration), `SharedMemoryWriter::updatePhysics` |
| `tyres[i].status.load` / `ndSlip` / `surfaceDef` | `Tyre::step` (#4) | from the **previous** step: the `allLoaded` / sleeping test and the vibration terms in `pollControls` (both run before `stepComponents`); from the **current** step: `updateColliderStatus` and everything in `stepComponents` after #4 |
| `Car::lastFF` and the wheel force | `Car::onTyresStepCompleted` (inside #4) | the controls provider; `Car::getFinalFF` 0x140270960 (`controlsProvider->getFFGlobalGain() * lastFF * userFFGain`; the virtual is slot +0x28 and is called with no null check), called by `SharedMemoryWriter::updatePhysics` |
| `splineLocatorData` | `Car::postStep` | AI, shared memory, comparative telemetry, test mode |
| `damageZoneLevel[5]` | `Car::onCollisionCallBack` | suspension `setDamage` (same function), test mode; other readers not traced |
| rigid bodies | every component through `IRigidBody` | `core->step` after all cars |

---

## 7. Port notes

**Effort: M** for this layer alone. The scheduler is small (a fixed-step accumulator, two queues,
one lock), and `Car::step` / `pollControls` / `postStep` are short and self-contained. The cost is
that the glue cannot be tested alone: it calls every component.

What must exist before it: the rigid-body core (`IRigidBody`, `IJoint`, `IPhysicsCore`), then every
component in section 3.4 in that order, `Track` (ray cast + surfaces), `SplineLocator` and
`TimeTransponder` for lap timing. For a first whole-car run, components 19-25 and 27-29 can be
stubs.

Things a deterministic port must decide:

- Run cars in list order on one thread. The original pool makes cross-car reads (AI awareness,
  slipstream, the 6 m pit test, `evOnLapCompleted` handlers) depend on thread timing.
- Keep the ordering quirks, they change numbers: controls and vibration use tyre loads and `accG`
  from the previous step; `valueCache.speed` is sampled before any car is stepped (so the
  `speed` exported in `CarPhysicsState` and shared memory is the speed before the integration,
  next to a post-integration `velocity`); steering force is computed after tyre 3, before aero
  and drivetrain; `Car::postStep` gets `dt = 0`.
- The ERP/CFM switch applies to car 0 only.
- Every stepped car needs a controls provider: `Car::step` calls `getAction(4)` through the
  pointer without a null check (5.3 step 4), and `PhysicsEngine::stepPaused` reads `cars[0]`
  without checking that the list is non-empty.
- If the pool is kept, never build it with one worker (5.2 step 6).
- `srand(ksGetSystemTime())` runs once on the physics thread. The `rand()` users below this
  layer are listed in open question 11 (AI driver per step; suspension constructors and dynamic
  track at load): they break reproducibility unless the oracle fixes the seed.

Bit-exact test with an oracle (same in-process trick as the tyre oracle):

- Skip `PhysicsDriveThread` completely. Build a `PhysicsEngine` (0x140262430), a track and one
  `Car` (0x14026bf00) in the mapped image, null the pool pointer (`PhysicsEngine + 0x1e0`) or use
  `THREADS=0`, then call `PhysicsEngine::step` 0x140264760 directly with
  `(0.003, t += 3.0, t)`.
- Fake: an `ICarControlsProvider` whose vtable returns scripted controls from `acquireControls`
  (+0x08), false from `getAction` (+0x10) and no-ops for `sendFF` / `setVibrations` /
  `setEngineRPM`; the config readers (`assetto_corsa.ini`, the car folder); a flat track mesh.
- Compare after each step with `Car::getPhysicsState` 0x140270d70 (one flat 0xb70 struct) plus the
  body state read through `IRigidBody`.
- Smaller units first: `Car::pollControls` 0x140274e70 with a fake provider and four `TyreStatus`
  blocks (pure arithmetic + `sinf`); `Car::onTyresStepCompleted` 0x140274cd0; the fuel burn and
  `accG` parts of `Car::step` by calling `Car::step` with `stepComponents` neutralised is not
  possible without patching, so those are better checked in the whole-car run.

---

## 8. Open questions

1. **Order of the `evOnStepCompleted` handlers.** The handlers run in registration order, which is
   object construction order at load (cars, remote cars, track objects, client). I listed them by
   type; the real interleaving was not traced.
2. **`physicsGUID == 0` means "the player's car"** is an interpretation: it is the index of the car
   in `PhysicsEngine::cars` at construction. That the local player is always created first was not
   checked.
3. **Replay playback path.** Resolved by the adversarial reviewer (5.1): the per-frame caller is
   `ReplayManager::updateReplayRecorders` 0x14015bc90 (from `ReplayManager::updatePlayState`
   0x14015b9a0, from `Sim::update`), which calls each recorder's own provider sub-object, not
   `CarAvatar::physicsStateProvider`. Still open: what reads `CarAvatar::physicsStateProvider`
   at all (by member name only the constructors, `initCommonPostPhysics`, `onStartReplay` and
   `onStopReplay` touch it), and what the car models show in replay while online, where physics
   keeps running and `PhysicsAvatar::update` keeps calling `CarAvatar::setNewPhysicsState` on
   the same `physicsState` that the replay writes.
4. **`Track::step` grip clamp.** Resolved by the reviewer from the disassembly: a plain
   `clamp(grip, 0.85, 1.0)` (5.2 step 3).
5. **`CutGas` penalty.** `Car::step` only runs the timer. Checked by the reviewer: a text search
   of all pseudo-C for `penaltyTime`, `penaltyTimeAccumulator`, `getPenaltyTime` and `penaltyMode`
   finds no reader in the engine, drivetrain or electronics. The hits outside `Car` are
   `TimeTransponder::onTimeLinePassed` (checks `Car::getPenaltyTime() <= 0` when the line is
   crossed), `LapInvalidator::step` / `onEnterPenaltyZone`, `ACClient::sendCarPosition` and one
   `ACClient` lambda, `CarAvatar::hasPenalty`, `RaceManager::setCurrentSession`, `TimeAttack` and
   the `PhysicsEngine` constructor (default `penaltyMode = Nothing`). So nothing cuts the gas:
   the name means "the timer only runs down while the driver himself holds gas below 0.1"
   (interpretation of the confirmed logic in 5.3 step 9).
6. **`getAction(4)`** is read as "headlight switch" from the member names (`lightsOn`,
   `lastLigthSwitchState`); the `DriverActions` enum was not read.
7. **`drivetrain.currentGear != 1`** in the sleeping test is read as "not in neutral" (0 = reverse,
   1 = neutral). Confirmed: `docs/map/drivetrain.md` 4.8 (`gears[1]` is neutral, ratio 0).
8. **`ksGetTime` / `ksGetQPTTime` units**: resolved, both are milliseconds. `ksGetTime`
   0x140238a30 returns `(QueryPerformanceCounter - startTime) * 1000 / frequency` when the global
   `isUsingQPT` is set, else `timeGetTime() - startTGT`; `ksGetQPTTime` 0x1402389d0 is always the
   first formula; `ksGetSystemTime` 0x140238a10 (the `srand` seed) is `timeGetTime()`. Who sets
   `isUsingQPT` was not read.
9. **Main-thread update order** (`Game::onIdle` 0x140242730 → `Game::update` 0x140243010 → the
   `GameObject` tree: `Sim::update`, `PhysicsAvatar::update`, `SharedMemoryWriter::update`,
   `RaceManager::update` 0x140141550, `ACPlugin::update`) was not traced; only the members of that
   list were confirmed as main-thread `update` methods by vtable slot or caller.
10. **Test-mode command-line switches**: resolved, see section 4 (`-test`, `-get_car_range`,
    `-get_car_bestlap`). What `-autodrive` sets was not read.
11. **`rand()` users**: searched by the adversarial reviewer (text search of all pseudo-C, 39
    files). On the per-step path only the AI driver uses it: `AIDriver::stepHumanizer`
    0x1402a19e0, `AIDriver::stepPushVariations` 0x1402a1f20, `AIDriver::stepGasBrakeStart`
    0x1402a1580 (plus its `onNewSession`, `generatePitStopStrategy`, `generatePitlaneWait`).
    At load time: the `Suspension`, `SuspensionStrut` and `SuspensionML` constructors,
    `Track::initDynamicTrack` 0x140278300, `AIDriver::AIDriver`,
    `RaceEngineer::findTyreCompound`; on request `Car::getPitstopTime` 0x140271e60. A
    human-driven car therefore has no `rand()` in its step, but its suspension constructors do
    draw from it. What they use it for belongs to the suspension map. Not checked: on which
    thread those constructors run (the MSVC `rand` state is per thread, and only the physics
    thread is seeded with `srand(ksGetSystemTime())`).
12. **`ISuspension` slot +0xb0 on `ACSuspension` and `TyreTester`** resolves to the shared empty
    function; they are not car suspensions and are ignored here.
13. **`Car::evOnTyreCompoundChanged` / `evOnPush2Pass` / `evOnControlsProviderChanged`**: raising
    sites now listed in 6.1. `evOnPush2Pass` **is** on the per-step path (`Engine::stepP2P`); what
    its handler does on a pool worker thread was not read.
14. **`THREADS=1`** (one pool worker) looks fatal from the arithmetic in `PhysicsEngine::step`
    (5.2 step 6). Not run; unverified. The same arithmetic wraps for any pool size when the car
    list is empty; whether `PhysicsEngine::step` can run before the first `Car` is constructed
    (the thread is started from `Sim::Sim`) was not checked. `stepPaused` and the test-mode
    block also read `cars[0]` unguarded.
15. **`param2` of the collision `ACPhysicsEvent`**: resolved, it is the ODE contact penetration
    depth (`dContactGeom.depth`, 5.7).
16. Chosen without asking: third-level callees are listed only for the drivetrain, aero and
    transponder rows, where they matter for port order; the other rows stop at two levels because
    their callees are leaf helpers.
17. **Jump start in drive-through mode** (5.3 step 17): by the code the penalty and
    `evOnJumpStartEvent` repeat every step while the car is off its grid spot before the start.
    Not run; whether a handler elsewhere (network client, `PenaltyManager::evOnPenalty`
    listeners) de-duplicates was not read. The numeric values of `JumpStartPenaltyMode` and which
    mode a session uses were not read either.
18. **Initial value of `ReplayRecorder::recordIntervalMS`** (used when `cfg/replay.ini` is
    missing or `LEVEL` is outside 0..4) is a static initialiser that was not read.
19. **`Car::setSlipStreamEffects` ignores its first argument** (gain stays 1.0, section 5.3
    step 6); the second one is used: `SlipStream::speedFactor = arg2 * 0.5`, fed from assists.ini
    `[ASSISTS] SLIPSTREAM`. Resolved in `docs/map/aero.md` 4.4 and 5.2: the setting only
    lengthens the other cars' wakes.

---

## 9. Execution order summary table

Thread: **PT** = physics thread, **W** = physics thread or a pool worker (cars past the first
chunk), **MT** = main thread. One row per call, in the order it happens within one step.

| # | Function | Address | Thread | Purpose |
|---|---|---|---|---|
| 1 | `PhysicsDriveThread::run` (or `::step` 0x140123560 from the timer) | 0x140123150 | PT | Turn real time into 3 ms steps. |
| 2 | `DirectInput::poll` + `DICommandManager::step` | 0x14023f160 / 0x140081730 | PT | Read human devices. |
| 3 | `PhysicsEngine::step` | 0x140264760 | PT | One 0.003 s step of the world. |
| 4 | `PhysicsAvatar::stepCommandQueue` | 0x1401223e0 | PT | Run jobs sent by the main thread. |
| 5 | `Track::step` | 0x140278d20 | PT | Dynamic grip; AI line recorder. |
| 6 | `PhysicsEngine::stepWind` | 0x140265380 | PT | Wind gusting. |
| 7 | `Car::stepPreCacheValues` (each car) | 0x1402768c0 | PT | Cache speed and power index. |
| 8 | `Car::step` (each car) | 0x140275da0 | W | Everything below up to row 22. |
| 9 | ERP/CFM switch (car 0) | inline | PT (car 0 is always in the physics thread's chunk) | Stiffer joints when stopped. |
| 10 | `Car::pollControls` | 0x140274e70 | W | Driver inputs; **AI runs here** (`AIDriver::acquireControls` 0x14029baa0); rumble out. |
| 11 | `Car::updateAirPressure` | 0x140276ae0 | W | Air density with slipstream. |
| 12 | fuel burn, `Car::updateBodyMass` | inline / 0x140276c70 | W | Fuel level; masses once per second. |
| 13 | control overrides, penalties, final steer angle | inline | W | Locks, gentle stop, penalty timers. |
| 14 | `Autoclutch::step` | 0x1402b9590 | W | Automatic clutch. |
| 15 | sleeping test, `accG` | inline | W | Freeze a resting car; g-forces. |
| 16 | `Car::stepThermalObjects` | 0x1402769f0 | W | Water temperature. |
| 17 | `Car::stepComponents` | 0x1402764d0 | W | The 29 component steps of section 3.4. |
| 18 | — `Tyre::step` x4 | 0x140283800 | W | Tyre forces into the hubs. |
| 19 | — `Car::onTyresStepCompleted` | 0x140274cd0 | W | Steering force feedback out. |
| 20 | — `TimeTransponder::step` | 0x1402911f0 | W | **Lap and sector timing**, lap events. |
| 21 | `Car::updateColliderStatus` | 0x140276df0 | W | Pit-lane ghosting, collision mask. |
| 22 | `Car::stepJumpStart` (car 0) | 0x140276780 | PT (car 0 only) | Jump-start detection. |
| 23 | spin until all pool tasks are done | inline | PT | Join. |
| 24 | `PhysicsCore::step` | 0x1402cd690 | PT | Collisions, then integrate. |
| 25 | — `PhysicsEngine::onCollisionCallBack` → `Car::onCollisionCallBack` | 0x140264020 / 0x140274650 | PT | **Collisions processed**: damage, event queue. |
| 26 | — `dWorldStep` | 0x1403404c0 | PT | Move all rigid bodies. |
| 27 | `Car::postStep` (each car) | 0x140275430 | PT | Track position data; ghost recorder. |
| 28 | `NetCarStateProvider::step` (each remote car) | 0x14011bd00 | PT | Advance remote cars. |
| 29 | `ACClient::onPhysicsStep` | 0x14004f1d0 | PT | Send / receive network car state. |
| 30 | test-mode checks | inline | PT | Automated test exits. |
| 31 | `PhysicsAvatar::onPhysicsStepCompleted` | 0x1401221a0 | PT | Publish the step. |
| 32 | — `PhysicsEngine::getPhysicsStates` → `Car::getPhysicsState` | 0x140263ac0 / 0x140270d70 | PT (locked) | Snapshot every car. |
| 33 | — `SharedMemoryWriter::updatePhysics` | 0x140186ef0 | PT | **Shared memory physics page written.** |
| 34 | — `ReplayManager::onStepCompleted` | 0x140158b50 | PT | **Replay recorded.** |
| 35 | `PhysicsAvatar::update` | 0x140122480 | MT (locked copy) | Once per frame: copy snapshots to the car models. |
| 36 | `Sim::stepPhysicsEvent` | 0x14019ebd0 | MT | Once per frame: collision events to game and audio. |
