# Telemetry link: shared memory

Source: `acs.exe` + `acs.pdb`, read through the local material only: pseudo-C in `re/decomp/`
(folders `SharedMemoryWriter`, `Car`, `CarAvatar`, `PhysicsAvatar`, `PhysicsEngine`, `RigidBodyODE`,
`SharedFileOut`, `RemoteTelemetryUDP`), struct layouts in `re/types/`, and `tools/disasm.py` for the
two writer functions (the pseudo-C of the per-wheel loop is garbled, so every offset in section 5 was
taken from the disassembly). Our logger `ac_telemetry.py` and the recorded lap `f2004_spa_ai.csv`
were read, the CSV also measured (script `re/car/telemetry/csv_stats.py`).
Raw material: `re/car/telemetry/`. Nothing in the game folder, the Ghidra project or git was
modified. No Rust was written.

Words used below: **page** = one named block of Windows shared memory; **physics thread** = the
thread that steps the cars 333 times a second; **main thread** = the thread that draws frames.

---

## 1. Plain-English summary

The game publishes three small blocks of memory that any other program on the PC may read:
one with fast-changing car numbers (`acpmf_physics`), one with lap and session numbers
(`acpmf_graphics`) and one with facts that never change during a session (`acpmf_static`).
One object, `SharedMemoryWriter`, creates all three when a session is loaded and fills them.
The car block is rewritten after **every physics step**, by the physics thread, straight from the
live car object: pedals, gear, engine speed, body speed, wheel loads, tyre temperatures and so on.
Almost every number is a plain copy of one variable; a few get a unit change (m/s to km/h,
joules to kilojoules, wear to a percentage) and the three body angles are computed from the body's
orientation.
Each rewrite carries a counter that goes up by one, so a reader can tell whether it has already
seen that step.
The lap block is rewritten once per **drawn frame** by the main thread, so its numbers (lap time,
position on track, world position) change only as often as the screen is redrawn (about 48 times
a second in our recording) and are repeated in between.
The static block is written once.
Nothing is locked while a block is being rewritten, and nothing in this system pushes forces into
the car: it only looks.
For us it is the window through which a recorded lap can be compared with a Rust car, so the exact
meaning, unit and timing of every number matters more than the code itself.
One warning up front: the recorded lap we have was very probably made with a third-party patch
loaded into the game, and at least one of its numbers (the air density) is not what the game's own
code computes (section 9.5).

---

## 2. Classes

| Class / struct | Size | What it is for |
|---|---|---|
| `SharedMemoryWriter` | 0x220 | The writer. A `GameObject` owned by `Sim`. Members: `sim` +0x58, `car` +0x60 (the physics `Car` of the player), `avatar` +0x68 (the render-side `CarAvatar`), `lapInvalidator` +0x70, `flagManager` +0x78, `sharedMemories[3]` +0x80, `physicsInfo` +0xc8 (a private copy of the car's `CarPhysicsInfo`), `lastPosition` +0x208, `staticWrote` +0x214, `currentDistance` +0x218, `hotlapMode` +0x21c. |
| `SharedMemoryElement` | 0x18 | One page: `hMapFile`, `mapFileBuffer`, `packetId`, `nullCounts` (a warm-up counter). Index 0 = static, 1 = graphics, 2 = physics. |
| `SPageFilePhysics` | 0x250 (592) | Layout of `acpmf_physics`. 4-byte packing, only `int32` and `float`. |
| `SPageFileGraphic` | 0x130 (304) | Layout of `acpmf_graphics`. Contains `wchar_t` strings, so some `int32` fields sit after 2 bytes of padding. |
| `SPageFileStatic` | 0x2ac (684) | Layout of `acpmf_static`. |
| `SPageFile` | 0x19c (412) | Older single-page layout, used only by `SharedFileOut` (page `acpmf`). |
| `CarPhysicsState` | 0xb70 | Flat snapshot of one car made by `Car::getPhysicsState` after every step for the render side and the replay. The physics page takes **only four things** from it (ride heights, limiter rpm, ERS-charging flag, push-to-pass bytes); the graphics page takes the three lap times. |
| `CarPhysicsInfo` | 0x140 | Constants of the car collected once in `CarAvatar::initPhysics` (max power, max fuel, bump-stop positions, tyre radius ...). Source of the static page and of the bump-stop offset used for `suspensionTravel`. The bump-stop positions are filled only for double-wishbone wheels (0 otherwise, section 5.3). |
| `CarAvatar` | 0x12a8 | Render-side car. Holds `physicsState` +0x268 (main-thread copy of the snapshot), `physicsInfo` +0xe40, `bodyMatrix` +0x224 and the cockpit settings (`currentEngineBrakeSetting` +0x127c, `isHeatChargingBatteries` +0x1280, `currentERSRecovery` +0x1288, `mandatoryPitstopDone` +0x1295). |
| `OnPhysicsStepCompleted` | 0x10 | Event argument `{double pt, vector<CarPhysicsState>* states}`. The writer uses neither field. |
| `SharedFileOut` | 0x98 | One line: legacy writer of the single page `Local\acpmf` (0x19c bytes, struct `SPageFile`), created unconditionally in `Sim::Sim`, filled per frame from `CarAvatar::physicsState`. |
| `RemoteTelemetryUDP` | 0x1d0 | One line: UDP telemetry server bound to port 9996 (`htons(0x270c)`), serviced per frame on the main thread (`listen` + `updateListeners`). |

---

## 3. Key functions

### Loading / init

```
Sim::Sim                                             0x140192070   (main thread, session load)
├─ SharedFileOut::SharedFileOut                      0x14018e7d0   legacy page "Local\acpmf"
├─ SharedMemoryWriter::SharedMemoryWriter            0x140185430
│  ├─ GameObject name "SharedMemoryWriter"; all packetId / nullCounts = 0
│  ├─ SharedMemoryWriter::initSharedMemory(0, "acpmf_static")     0x140185f70
│  ├─ SharedMemoryWriter::initSharedMemory(1, "acpmf_graphics")
│  ├─ SharedMemoryWriter::initSharedMemory(2, "acpmf_physics")
│  │     name = "Local\" + argument
│  │     CreateFileMappingW(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 0x250, name)
│  │     MapViewOfFile(handle, FILE_MAP_ALL_ACCESS (0xf001f), 0, 0, 0x250)
│  │     failure → MessageBox "CreateFileMapping failed" / "MapViewOfFile failed"
│  │     (the second test looks at the handle again, not at the view pointer, so a failed
│  │      MapViewOfFile is in fact never reported)
│  ├─ sim->physicsAvatar->evOnStepCompleted += lambda → SharedMemoryWriter::updatePhysics
│  │     (Event::addHandler 0x140155860 is a plain push_back: handlers run in registration order)
│  ├─ sim->evNewCarLoaded += lambda (operator() at 0x140185d70):
│  │     for the car with CarAvatar::getGuid() == 0 and a RaceEngineer:
│  │     avatar = that car; physicsInfo = avatar->physicsInfo (CarPhysicsInfo::operator= 0x140185ab0);
│  │     car = the RaceEngineer's Car*; lapInvalidator = &car->lapInvalidator
│  └─ reads Documents cfg/race.ini (section 4) → hotlapMode
└─ RemoteTelemetryUDP::RemoteTelemetryUDP            0x1401468e0   UDP port 9996

CarAvatar::initPhysics                               0x1400d7660   fills CarAvatar::physicsInfo
SharedMemoryWriter::shutdown                         0x140186140   (GameObject slot +0x28) writes a zeroed
                                                                   graphics page: status = 0 (off)
```

### Every physics step (physics thread)

```
PhysicsDriveThread::run                              0x140123150
└─ per 3 ms step:
   ├─ PhysicsEngine::step(0.003, ...)                0x140264760
   │    Car::stepPreCacheValues 0x1402768c0 (caches the speed), Car::step 0x140275da0,
   │    PhysicsCore::step 0x1402cd690 (ODE), engine evOnStepCompleted (Car::postStep 0x140275430)
   └─ evPhysicsStepCompleted → PhysicsAvatar::onPhysicsStepCompleted   0x1401221a0
        ├─ lock mutex; PhysicsEngine::getPhysicsStates 0x140263ac0
        │     └─ Car::getPhysicsState (each car)     0x140270d70   → PhysicsAvatar::currentStates
        ├─ unlock
        └─ PhysicsAvatar::evOnStepCompleted handlers, in registration order. The writer is
           FIRST: Sim::Sim constructs SharedMemoryWriter before ReplayManager (whose constructor
           registers the replay recorder's handler). The dev app FormTelemetry also registers
           here, in its own constructor (when that runs was not traced).
             SharedMemoryWriter::updatePhysics       0x140186ef0
               ├─ return if car == null
               ├─ if sharedMemories[2].nullCounts < 300: nullCounts++; return     (warm-up)
               ├─ local SPageFilePhysics zeroed (memset 0x250)
               ├─ packetId = sharedMemories[2].packetId++  (value before the increment)
               ├─ direct reads of Car members (section 5.3)
               ├─ Car::getSpeed 0x140272160, Drivetrain::getEngineRPM 0x140266b80,
               │  BrakeSystem::getFrontBias 0x14028d630
               ├─ per wheel: tyre status, ISuspension::getStatus (+0x48, e.g. Suspension::getStatus
               │  0x1402c1bf0), Curve::getValue 0x140206990 (wear curve),
               │  TyreThermalModel::getIMO 0x1402addb0, brake disc temperature, contact point/normal
               ├─ Car::getVelocity 0x140272940, Car::getLocalVelocity 0x140270d20
               ├─ body->getWorldMatrix (RigidBodyODE::getWorldMatrix 0x1402ce6c0) → atan2f / asinf
               ├─ Car::getCGHeight 0x1402703b0, Car::getLocalAngularVelocity 0x140270cf0,
               │  Car::getFinalFF 0x140270960, PerformanceMeter::getCurrentSplit 0x14026b700
               ├─ avatar->physicsState.rideHeight / limiterRPM, cockpit settings from avatar
               ├─ Car::getControlsProvider 0x140270650 + dynamic_cast<AIDriver*> → isAIControlled
               └─ copy the 0x250 local bytes into the page, 16 bytes at a time, lowest address first
```

### Every drawn frame (main thread)

```
Game::update                                         0x140243010   walks all GameObjects (slot +0x08)
├─ PhysicsAvatar::update                             0x140122480   copies currentStates under the mutex,
│    └─ CarAvatar::setNewPhysicsState                0x1400da420   → CarAvatar::physicsState (per frame)
└─ SharedMemoryWriter::update                        0x140186230
     ├─ SharedMemoryWriter::writeStatic              0x1401877c0   once: when avatar, car set and !staticWrote
     ├─ if sharedMemories[1].nullCounts < 30: lastPosition = car 0 body position; nullCounts++; return
     ├─ return if avatar == null
     ├─ local SPageFileGraphic zeroed (memset 0x130); sharedMemories[1].packetId++   (first increment)
     ├─ status, lap strings (RaceManager::getBestLap 0x140137c00 / getLastLap 0x1401387c0 /
     │  getCurrentLap 0x1401380b0 + timeToString 0x140053110), RaceManager::getLapCount 0x140138570,
     │  session type, flag, position, lap times from avatar->physicsState
     ├─ packetId field = sharedMemories[1].packetId; then packetId++                 (second increment)
     ├─ tyre compound name, car->splineLocatorData.npos, body world position (3 getWorldMatrix calls),
     │  distance travelled, pit flags, wind, grip
     └─ copy the 0x130 local bytes into the page
```

---

## 4. Data it reads

No car data file is read by this system. It reads live objects only.

| File | Section / key | Goes to | Default | Notes |
|---|---|---|---|---|
| Documents `cfg/race.ini` (`INIReaderDocuments`) | `[SESSION_0] SPAWN_SET` | `SharedMemoryWriter::hotlapMode = true` when the value is `HOTLAP_START` | `false` | Set in the constructor. No reader of `hotlapMode` exists: the member name appears in no other function of `re/decomp`. |
| `changelog.txt` (game folder, `basic_ifstream<wchar_t>`) | first line(s) | `SPageFileStatic::acVersion` | — | Read in `writeStatic`; parsing not followed. |
| literal `L"1.7"` | — | `SPageFileStatic::smVersion` | — | Shared-memory interface version of this build. |
| `system/cfg/assetto_corsa.ini` | `[THREADING] USE_TIMER_PROCESS`, `[PHYSICS_THREADING] THREADS` | not read here | — | They decide which thread steps the car (see `docs/map/car_step.md`); the physics page is written on that thread. |

Per-car optional sections: none apply. Version switches: none. The page layout is fixed by the
build (`smVersion` 1.7).

---

## 5. Each step: inputs, maths, outputs

### 5.1 The three pages

| Page name (Windows object) | Index | Struct | Struct size | Size passed to `CreateFileMappingW` / `MapViewOfFile` | Written by | When |
|---|---|---|---|---|---|---|
| `Local\acpmf_static` | 0 | `SPageFileStatic` | 0x2ac (684) | 0x250 (592) | `writeStatic` | once, first frame where car and avatar exist |
| `Local\acpmf_graphics` | 1 | `SPageFileGraphic` | 0x130 (304) | 0x250 | `update` | every drawn frame, main thread |
| `Local\acpmf_physics` | 2 | `SPageFilePhysics` | 0x250 (592) | 0x250 | `updatePhysics` | every physics step, physics thread |

- All three are created with the **same size constant 0x250**, the size of the physics struct.
  The static struct is 92 bytes longer than that. It still works because Windows rounds a mapping
  up to whole 4096-byte pages; `writeStatic` writes all 0x2ac bytes. (Confirmed: `memset(…, 0x2ac)`
  and the copy loop; the mapping size is the literal in `initSharedMemory`.)
- The mappings are anonymous (backed by the page file), read/write, created by the game. A reader
  that opens them read-only, like our logger, cannot disturb them.
- `shutdown` writes a graphics page with `status = 0` and `session = 0`, the current `packetId`
  and `timeLimitSessionLeft = -1`, so `status` reads 0 (off) after the session ends. It does
  **not** clear its local struct first (no `memset`; checked in the disassembly): it zeroes the
  numeric fields 0x084 … 0x0af, 0x0f4 … 0x0fb and 0x108 … 0x127 and `isEscMenuVisible`, but the
  four lap-time strings (0x00c … 0x083), `tyreCompound` (0x0b0 … 0x0f3) and `carCoordinates`
  (0x0fc … 0x107) are copied from uninitialised stack, so they hold garbage after a session
  ends. The physics page is **not** cleared. The destructor (`SharedMemoryWriter::~SharedMemoryWriter` 0x140185810) unmaps
  and closes all three mappings, but a Windows mapping lives on while any program still has it
  open, and a new `CreateFileMappingW` with the same name then re-opens the old object with its
  old bytes (Windows behaviour, my interpretation of what the lap shows). So a reader that stays
  attached across a session change sees the last step of the old session until the new writer has
  finished its warm-up. The first row of `f2004_spa_ai.csv` (packetId 1790, followed by
  packetId 1) is such a stale page.

### 5.2 Physics page: when, how packetId advances, atomicity

- **Trigger (confirmed):** the lambda registered in the constructor on
  `PhysicsAvatar::evOnStepCompleted` calls `updatePhysics`. That event is raised by
  `PhysicsAvatar::onPhysicsStepCompleted`, which the physics thread calls after every
  `PhysicsEngine::step`. It is not the render update. One page write per 3 ms step.
- **Warm-up:** the first 300 calls with a car (0.9 s of physics) only count
  `sharedMemories[2].nullCounts` up. No page is written during them.
- **packetId:** the page gets the counter value, then the counter is incremented. So the first
  page written has `packetId = 0`, the next 1, and so on, exactly +1 per physics step that ran.
  The page with `packetId = p` is written by call number `p + 301` after the car was attached
  (300 warm-up calls first). Our logger skips `packetId == 0`, so its first row is the second page.
- **Paused / replay:** the physics thread does not step while paused, so the page and `packetId`
  freeze. (The physics thread is also paused in replay mode, see `car_step.md`.)
- **Can steps be missed by a reader? Yes.** The writer never waits for a reader. After a stall the
  physics thread runs up to about 333 catch-up steps back to back, each overwriting the page within
  microseconds. A polling reader then sees `packetId` jump. In the recorded lap 117,332 steps were
  seen and 78 were missed, in 38 gaps (29 of one step, 5 of two, the longest 16).
- **Is the page written atomically? No.** The struct is built in a local buffer and copied with
  `movups` pairs, 16 bytes at a time from offset 0 upward, with no lock, no interlocked operation
  and no "being written" flag. `packetId` is in the **first** 16-byte chunk (with gas, brake, fuel).
  A reader that copies the page while it is being written can get a new `packetId` with old values
  further up, or the reverse. The copy is only 37 moves of 16 bytes and the logger takes one
  `memcpy` snapshot of its 312-byte prefix, so torn rows should be rare, but they are possible and cannot
  be detected from one read. A reader that wants certainty must read twice and keep the row only
  when both copies are equal.
- **Order inside the writer:** all values are read in one call on the physics thread after the
  step, so the fields of one page belong together, with two exceptions that come from the main
  thread's copy and are up to one drawn frame old: `rideHeight[2]` and `currentMaxRpm` (plus the
  ERS/P2P cockpit values). In the recorded lap `rideHeight_0` changes 47 times a second while
  `wheelLoad_fl` changes 328 times a second, which matches.

### 5.3 Physics page: every field the logger records

`car` = `SharedMemoryWriter::car` (the physics `Car`), `T[i]` = `car->tyres[i]`, wheel order
0 = front left, 1 = front right, 2 = rear left, 3 = rear right. "Raw" = copied bit for bit from
the variable named. "CarPhysicsState twin" names the member of the per-step snapshot that carries
the same quantity and says where it differs; the page itself does **not** go through the snapshot
unless stated.

| Field | Offset | Expression in `updatePhysics` | Comes from | Units / conversion | Instantaneous? | CarPhysicsState twin |
|---|---|---|---|---|---|---|
| `packetId` | 0x000 | `sharedMemories[2].packetId`, then `++` | the writer's own counter | count of pages | — | none |
| `gas` | 0x004 | `car->controls.gas` | `Car::controls` (`CarControls` +0x24), filled each step by `Car::pollControls` from the controls provider (AI or player). Later in the same step `AutoBlip::step` and `AutoShifter::step` (gas cut) and `Car::step` itself (forced stop cases) can overwrite it; the page shows the value left at the end of the step | 0..1 | this step's value | `gas` (same) |
| `brake` | 0x008 | `car->controls.brake` | `CarControls` +0x28 | 0..1 | this step | `brake` (same) |
| `fuel` | 0x00c | `(float) car->fuel` | `Car::fuel` (double, +0x3db8) | same unit as car.ini fuel (litres; `Car::fuelKG` is a separate member) | this step | `fuel` (same) |
| `gear` | 0x010 | `car->drivetrain.currentGear` | `Drivetrain::currentGear` (+0x584 in `Drivetrain`) | raw index: 0 = reverse, 1 = neutral, 2 = first gear. The logger prints `gear - 1`; the CSV column is raw | this step (the engaged gear, not the request) | `gear` (same) |
| `rpms` | 0x014 | `(int) Drivetrain::getEngineRPM()` | `(float)drivetrain.engine.velocity * 0.15915507 * 60.0` (`engine` = `GearElement`, angular speed in rad/s) | rev/min, truncated toward zero. Not clamped: can be negative | this step | `engineRPM` (float, clamped at 0) |
| `steerAngle` | 0x018 | `car->controls.steer` | `CarControls` +0x2c | **not an angle**: steering input −1..+1 of full lock. Steering-wheel degrees = `steer * Car::steerLock`; road-wheel signal = `steerLock * steer / steerRatio` (`Car::finalSteerAngleSignal`) | this step | `steer` = `controls.steer * steerLock` (degrees) |
| `speedKmh` | 0x01c | `Car::getSpeed().value * 3.6f` | `Car::valueCache.speed` = length of the body's linear velocity, **cached at the start of the step** by `Car::stepPreCacheValues` | km/h (constant `0x40666666` = 3.5999999) | see section 5.5 | `speed` (m/s, same cached value) |
| `velocity[3]` | 0x020 | `Car::getVelocity()` → `body->getVelocity()` | ODE linear velocity of `Car::body` (centre of mass) | m/s, **world** axes x, y (up), z | see section 5.5 | `velocity` (same call) |
| `accG[3]` | 0x02c | `car->accG` | `Car::step`: `worldToLocalNormal((v − lastVelocity) * (1/dt) * 0.101978384)` | g (1 g = 9.806 m/s²), **car-local** axes: x lateral, y vertical, z longitudinal (+z = forward: braking is negative z). Change of velocity only, gravity not included: 0 at rest | one-step difference, not filtered | `accG` (same) |
| `wheelSlip[4]` | 0x038 | `T[i].status.ndSlip` | tyre model output (`TyreStatus` +0x2c) | normalised combined slip: 1.0 = slip at peak grip. **Not** slip ratio and not slip angle | this step | `ndSlip` (same). The twin called `tyreSlip` is `status.slipFactor` |
| `wheelLoad[4]` | 0x048 | `T[i].status.load` | `Tyre::addGroundContact` (`TyreStatus` +0x04) | N, ≥ 0 | this step | `load` (same) |
| `wheelsPressure[4]` | 0x058 | `T[i].status.pressureDynamic` | `TyreStatus` +0x60 | psi (hot pressure) | this step | inside `tyreThermalStates` (not checked) |
| `wheelAngularSpeed[4]` | 0x068 | `T[i].status.angularVelocity` | `TyreStatus` +0x14 | rad/s, sign as stored (forward rolling is positive in the recorded lap) | this step | `wheelAngularSpeed`: set to 0 when `status.isLocked` or the car is sleeping, otherwise the same |
| `tyreWear[4]` | 0x078 | `Curve::getValue(T[i].modelData.wearCurve, (float)T[i].status.virtualKM) * 100.0` | wear curve of the compound looked up at the tyre's virtual kilometres | wear-curve value times 100; 99.5 … 100 in the recorded lap | this step | `wear` = `min(virtualKM / maxWearKM, 1)` (or −1): a different quantity |
| `tyreDirtyLevel[4]` | 0x088 | `T[i].status.dirtyLevel` | `TyreStatus` +0x40 | 0..(level cap), unitless | this step | `tyreDirtyLevel` (same) |
| `tyreCoreTemperature[4]` | 0x098 | `T[i].thermalModel.coreTemp` | `TyreThermalModel::coreTemp` (`Tyre` +0x45c) | °C | this step | inside `tyreThermalStates` |
| `camberRAD[4]` | 0x0a8 | `T[i].status.camberRAD` | `TyreStatus` +0x08 | radians, raw sign: left and right wheels have opposite signs for the same lean (CSV: −0.0528 FL, +0.0537 FR) | this step | `camberRAD`: **sign flipped for wheels 1 and 3** |
| `suspensionTravel[4]` | 0x0b8 | `suspensions[i]->getStatus().travel − physicsInfo.bumpStopsDn[i]` | `SuspensionStatus::travel` minus `ISuspension::bumpStopDn` (+0x20), copied at load by `CarAvatar::initPhysics`. **Only for double-wishbone wheels**: `initPhysics` does `dynamic_cast<Suspension*>` on each `ISuspension` and stores 0 for both bump stops when the cast fails (`SuspensionStrut`, `SuspensionAxle`, `SuspensionML` derive from `ISuspension`, not from `Suspension`). So on STRUT / AXLE / multilink wheels the page shows the raw `status.travel` | m. On a DWB wheel, because `bumpStopDn = −BUMPSTOP_DN` (see `suspension.md`), this is the travel measured from the droop stop: always positive in normal running. The F2004 is DWB front and rear | this step | `suspensionTravel` = `status.travel` without the offset |
| `drs` | 0x0c8 | `(float)(car->drs.isPresent && car->drs.isActive)` | `DRS` +0x00, +0x01 | 0.0 or 1.0 | this step | mask 0x4 of `statusBytes` |
| `tc` | 0x0cc | `car->tractionControl.isActive ? car->tractionControl.slipRatioLimit : 0` | `TractionControl` +0x01, +0x04 | the configured slip-ratio limit (0.08 on the F2004) while TC is switched on, else 0. It does not say TC is cutting power (`isInAction` is not exported). `isPresent` is not tested | this step | none |
| `heading` | 0x0d0 | `atan2f(−M31, M33)` of `body->getWorldMatrix(tmp, 0)` | third row of the body matrix = the car's forward (+z) axis in world coordinates | radians, −π..π. `heading = atan2(−forward.x, forward.z)`. If `M12 == 0 && M22 == 0` it is set to 0 | see 5.5 | from `worldMatrix` (the main-thread copy is time-smoothed) |
| `pitch` | 0x0d4 | `asinf(clamp(M32, −1, 1))` | forward axis, world y component | radians, positive = nose up | see 5.5 | — |
| `roll` | 0x0d8 | `atan2f(−M12, M22)`; degenerate case `atan2f(M21, M11)` | world-y components of the car's x and y axes | radians | see 5.5 | — |
| `cgHeight` | 0x0dc | `Car::getCGHeight()` = `body->getPosition().y − 0.25 * Σ T[i].contactPoint.y` | body position (centre of mass) and the four contact points of this step | m, world y difference (not along the ground normal) | this step | `cgHeight` (same formula) |
| `carDamage[5]` | 0x0e0 | `car->damageZoneLevel[0..4]` | `Car::damageZoneLevel` (+0x3de0), written by the collision / damage code (see the body map; not traced here) | internal damage level per zone (front, rear, left, right, centre is the public description; not verified here) | this step | `damageZoneLevel` (same) |
| `numberOfTyresOut` | 0x0f4 | `lapInvalidator->currentTyresOut` (skipped when the pointer is null) | `LapInvalidator` +0x10, `LapInvalidator::step` (player car only) | count 0..4 | this step | none |
| `pitLimiterOn` | 0x0f8 | `car->speedLimiter.isLimiting` | `SpeedLimiter` +0x01, set by `SpeedLimiter::step` 0x1402bb910 | 0 / 1. It is 1 only while a tyre stands on a pit-lane surface **and** the cached speed is above 80 km/h (the limiter is cutting the engine), not "limiter armed" | this step | none |
| `abs` | 0x0fc | `abs.isPresent && abs.isActive ? abs.slipRatioLimit : 0` | `ABS` +0x00, +0x01, +0x04 | slip-ratio limit while ABS is switched on, else 0 | this step | none |
| `kersCharge` | 0x100 | ERS car: `(float)ers.charge`; KERS car: `kers.charge`; else 0 | `ERS::charge` (double) / `Kers::charge` | 0..1 | this step | `kersCharge` (same) |
| `kersInput` | 0x104 | `ers.input` / `kers.input` / 0 | `ERS::input` / `Kers::input` | 0..1 | this step | `kersInput` (same) |
| `autoShifterOn` | 0x108 | `car->autoShift.isActive` | `AutoShifter` +0x00 | 0 / 1 | this step | none |
| `rideHeight[2]` | 0x10c | `avatar->physicsState.rideHeight[0..1]` | **the snapshot**, main-thread copy. `Car::getPhysicsState`: `Car::computeRideHeight(i, groundPlane)` 0x14026fc90 when the speed is ≥ 2 m/s or the setup has no minimum height, else `RaceEngineer::evalFrontRideHeight` / `evalRearRideHeight` (a static estimate) | m; front, rear. Height of `Car::ridePickupPoint[i]` above the plane through the first three tyres' unmodified contact points, measured along world y | **up to one drawn frame old** | `rideHeight` (this is it) |
| `turboBoost` | 0x114 | `car->drivetrain.acEngine.status.turboBoost` | `EngineStatus` +0x10 | boost in bar; 0 on the F2004 | this step | `turboBoost` (same) |
| `ballast` | 0x118 | `car->ballastKG` | `Car::ballastKG` (+0x3e30) | kg | constant | none |
| `airDensity` | 0x11c | `car->aeroMap.airDensity` | `AeroMap` +0x30, written every step by `Car::updateAirPressure` 0x140276ae0 (in `Car::step`): `PhysicsEngine::getAirDensity()` = `1.2922 − 0.0041 * ambientTemperature`, lowered by other cars' slipstream (`aero.md` 5.2) | kg/m³. By this code a car alone at 14 °C reads a constant 1.2348. **The recorded lap does not**: it reads 1.1671 at rest and moves between 1.1575 and 1.1717 with the car's height (section 9.5) | this step | `airDensity` (same) |
| `airTemp` | 0x120 | `car->ksPhysics->ambientTemperature` | `PhysicsEngine` +0x100 | °C | constant in a session | none |
| `roadTemp` | 0x124 | `car->ksPhysics->roadTemperature` | `PhysicsEngine` +0x104 | °C | constant in a session | none |
| `localAngularVel[3]` | 0x128 | `Car::getLocalAngularVelocity()` → `body->getLocalAngularVelocity()` = `worldToLocalNormal(getAngularVelocity())` | ODE angular velocity of `Car::body` | rad/s, car-local axes (y = yaw). In the lap, heading decreases when `localAngularVel_y` is positive | see 5.5 | `localAngularVelocity` (same call) |
| `finalFF` | 0x134 | `Car::getFinalFF()` = `controlsProvider->getFFGlobalGain() * (car->lastFF * car->userFFGain)` (this float order, from the disassembly: the two car members are multiplied first) | `Car::lastFF` from the steering force-feedback code (`steering.md`); provider slot +0x28 | force-feedback units (the lap spans −4.2 … +6.3) | this step | `lastFF_Final` (same product in the same order, so the same bits), `lastFF_Pure` |

### 5.4 Graphics fields the logger records

All written by `SharedMemoryWriter::update` on the main thread, once per drawn frame.

| Field | Offset | Expression | Comes from | Units / notes |
|---|---|---|---|---|
| `status` | 0x004 | `ReplayManager::isInReplaymode(sim->replayManager) ? 1 : (sim->pauseMenu->visible ? 3 : 2)` | byte at +0x150 of `sim->pauseMenu` = `ksgui::Control::visible` (confirmed in `re/types/ksgui_Control.txt`; `PauseMenu` has `ksgui::Control` as its base at +0) | 1 replay, 2 live, 3 pause. 0 only after `shutdown` or before the first write |
| `completedLaps` | 0x084 | `RaceManager::getLapCount(avatar->sim->raceManager, avatar)` | race rules on the main thread, not `TimeTransponder::lapCount` | laps. Not updated at the same instant as `iLastTime`: in the last CSV row `lastLapMs` already shows the third lap while `lap` is still 2 |
| `iCurrentTime` | 0x08c | `avatar->physicsState.lapTime` | snapshot ← `car->transponder.t` (`TimeTransponder` +0x00) | ms, unsigned. One frame old at most |
| `iLastTime` | 0x090 | `avatar->physicsState.lastLap` | snapshot ← `car->transponder.lastLap` | ms |
| `normalizedCarPosition` | 0x0f8 | `car->splineLocatorData.npos` | read **live** from the physics car (+0x3ad0); written by `Car::postStep` from `SplineLocator::normalizedPos` | 0..1 along the track's AI line |
| `carCoordinates[3]` | 0x0fc | `body->getWorldMatrix(tmp, 0)` → `M41`, `M42`, `M43`, **three separate calls** | ODE position of `Car::body` (centre of mass), read live from the main thread | m, world x, y (up), z. The three calls are not protected against the physics thread, so the three numbers can come from two neighbouring steps (error ≤ one step of motion, about 0.25 m at 300 km/h) |

Notes:
- `packetId` of the graphics page is incremented **twice** per frame (once at the top of the
  block, once after it is copied into the struct), so a reader sees 1, 3, 5, ... It still changes
  exactly once per drawn frame.
- The first 30 calls only warm up (`nullCounts < 0x1e`).
- In the recorded lap these values change 47 times a second on average (the frame rate of that
  session; it wanders between about 37 and 62 over the run); each is repeated in about 7
  consecutive CSV rows (up to 20).

### 5.5 Timing of the body values inside one packet (code versus recorded lap)

The code order inside one step is: `Car::stepPreCacheValues` (caches the speed) → `Car::step`
(computes `accG`, then tyres and everything else, which push forces into the bodies) →
`PhysicsCore::step` (ODE moves the bodies) → `updatePhysics`. Read literally, `velocity`,
`heading`, `pitch`, `roll`, `localAngularVel` and the body height in `cgHeight` would be the state
**after** this step's integration, while `speedKmh` and `accG` would describe the state **before**
it (one step older).

The recorded lap says otherwise, in three independent ways:

1. Over 117,293 consecutive packet pairs, `speedKmh` equals `3.6 * |velocity|` of the **same**
   packet (mean difference 4e-5 km/h, i.e. rounding), and differs from the previous packet's by up
   to 0.5 km/h.
2. `accG` of packet k equals `(velocity[k] − velocity[k−1]) / 0.003 / 9.806` (checked on a braking
   zone: magnitudes 2.094 g and 2.584 g both ways).
3. While the car waits in neutral before the launch (packets 1493 … 1504) `velocity` is exactly
   (0, 0, 0) and `accG` is 0, but `speedKmh` flickers around 0.005. `Car::step` zeroes the body
   velocity each step in that "sleeping" state (`RigidBodyODE::stop` 0x1402cead0) *after* the
   speed was cached.

So in practice **one packet = the body state that this step's `Car::step` worked with** (the state
before this step's forces are integrated) **plus the Kunos-side state after this step** (controls,
tyres, suspension travel, engine). The reaction of the body to the tyre forces of packet k shows
up in `velocity` of packet k+1 (visible at brake onset: wheel speed drops in packet 7540, `wheelSlip`
rises in 7541, the car starts to slow in 7542).
Why the writer does not see the post-integration velocity is **not explained by the code read**
(section 8, question 1). The rule above is what the data supports and is the one to use for
comparisons **with this recording**.

Review check (second pass, script `re/scratch/verify_telemetry/timing.py` / `timing2.py`): the
three observations were reproduced independently (115,238 consecutive packets above 50 km/h:
`speedKmh` against `3.6 * |velocity|` of the same packet differs by 1.3e-5 km/h on average,
against the previous or the next packet by 0.073; `|accG|` against the velocity difference to the
previous packet by 1.1e-4 g, against either neighbouring difference by 0.048 g). One more fact was
added: `heading[k] − heading[k−1]` equals `−0.003 * localAngularVel_y[k]` of the **same** packet
(residual 5.7e-6 rad, the rounding; 1.7e-5 with the angular velocity of either neighbour). So
orientation and angular velocity in a packet are one consistent body state; only its place
relative to `speedKmh` / `accG` contradicts the code order. The code order itself was re-read and
stands: `PhysicsEngine::step` calls `stepPreCacheValues`, `Car::step`, then core slot +0x10 =
`PhysicsCore::step` (`collisionStep`, `dWorldStep`), and only after it returns does
`PhysicsDriveThread::run` raise the event that reaches the writer; `valueCache.speed` has no
other writer than `stepPreCacheValues`; `PhysicsDriveThread::step` 0x140123560 (the timer-process
variant) has the same order. The most likely explanation is outside `acs.exe`: the lap was
recorded with a third-party patch loaded into the game (section 9.5), which is known from the
recording itself to change at least the air density. **An unpatched `acs.exe` (the oracle) may
therefore follow the code order**: `velocity`, the angles and `localAngularVel` one step newer
than `speedKmh` and `accG`. Measure it in the oracle before relying on either rule.

### 5.6 Remaining fields of the physics page

| Field | Offset | Source |
|---|---|---|
| `performanceMeter` | 0x138 | `(float) PerformanceMeter::getCurrentSplit(&car->performanceMeter).t` |
| `engineBrake` | 0x13c | `avatar->currentEngineBrakeSetting` (+0x127c; the pseudo-C mislabels it) |
| `ersRecoveryLevel` | 0x140 | ERS car: `avatar->currentERSRecovery` (+0x1288); else 0 |
| `ersPowerLevel` | 0x144 | ERS car: `CarAvatar::getERSPower(avatar).first` (0x1400d33e0); else 0 |
| `ersHeatCharging` | 0x148 | ERS car: `avatar->isHeatChargingBatteries` (+0x1280) |
| `ersIsCharging` | 0x14c | ERS car: `avatar->physicsState.kersIsCharging` (snapshot ← `ers.isCharging`) |
| `kersCurrentKJ` | 0x150 | `ers.currentJ * 0.001` or `kers.currentJ * 0.001` or 0 |
| `drsAvailable` | 0x154 | `drs.isPresent ? drs.isAvailable : 0` |
| `drsEnabled` | 0x158 | `drs.isPresent ? drs.isActive : 0` |
| `brakeTemp[4]` | 0x15c | `car->brakeSystem.discs[i].t` (`BrakeDisc` +0x00, stride 0x90) |
| `clutch` | 0x16c | `car->controls.clutch` (`CarControls` +0x30). **Not in the logger's prefix** |
| `tyreTempI/M/O[4]` | 0x170 / 0x180 / 0x190 | `TyreThermalModel::getIMO(&T[i].thermalModel, out[3])` → out[0], out[1], out[2] |
| `isAIControlled` | 0x1a0 | `dynamic_cast<AIDriver*>(car->getControlsProvider()) != null` |
| `tyreContactPoint[4][3]` | 0x1a4 | `T[i].contactPoint` (world) |
| `tyreContactNormal[4][3]` | 0x1d4 | `T[i].contactNormal` |
| `tyreContactHeading[4][3]` | 0x204 | `T[i].roadHeading` |
| `brakeBias` | 0x234 | `BrakeSystem::getFrontBias()` = `biasOverride == −1 ? frontBias : biasOverride` |
| `localVelocity[3]` | 0x238 | `Car::getLocalVelocity()` → `body->getLocalVelocity()` (car-local m/s) |
| `P2PActivations` | 0x244 | P2P car: `avatar->physicsState.p2pActivations`; else 0 |
| `P2PStatus` | 0x248 | P2P car: `avatar->physicsState.p2pStatus` (3 active; 2 available; 1 not available: cooling down or none left) |
| `currentMaxRpm` | 0x24c | `avatar->physicsState.limiterRPM` (snapshot ← `Engine::getLimiterRPM` 0x140285a50) |

### 5.7 Remaining fields of the graphics page

| Field | Offset | Source |
|---|---|---|
| `packetId` | 0x000 | section 5.4 note |
| `session` | 0x008 | `RaceManager::getCurrentSessionType`: 0 practice, 1 qualify, 2 race, 3 hotlap, 4 time attack, 5 drift, 6 drag, −1 other |
| `currentTime`, `lastTime`, `bestTime` (`wchar_t[15]`) | 0x00c, 0x02a, 0x048 | `timeToString(lap.time, 3)` of `RaceManager::getCurrentLap` / `getLastLap` / `getBestLap`; a fixed literal when longer than 14 characters |
| `split` (`wchar_t[15]`) | 0x066 | `timeToSectorString(last split of the current lap, 1)` or a fixed literal when there is none |
| `position` | 0x088 | `RaceManager::getCarLeaderboardPosition` 0x140137ee0 |
| `iBestTime` | 0x094 | `avatar->physicsState.bestLap` (← `transponder.bestLap`) |
| `sessionTimeLeft` | 0x098 | `(float) RaceManager::getSessionTimeLeft()` 0x140139e60 |
| `distanceTraveled` | 0x09c | `currentDistance += |lastPosition − avatar->bodyMatrix translation|` per frame (render matrix; m) |
| `isInPit` | 0x0a0 | `CarAvatar::isInPit` 0x1400d8bd0: snapshot position within 1.5 m of the pit box |
| `currentSectorIndex` | 0x0a4 | number of splits in the current lap |
| `lastSectorTime` | 0x0a8 | last split of the current lap, or of the last lap when the current lap has none |
| `numberOfLaps` | 0x0ac | `RaceManager::getCurrentSession().laps` |
| `tyreCompound` (`wchar_t[33]`) | 0x0b0 | `CarAvatar::getTyreCompound(avatar, 0, true)` 0x1400d3980 |
| `replayTimeMultiplier` | 0x0f4 | `sim->replayManager->timeMult` |
| `penaltyTime` | 0x108 | `(float) CarAvatar::hasPenalty(avatar)` 0x1400d3b70 |
| `flag` | 0x10c | `FlagManager::getCurrentFlag` 0x14009cc20 remapped: 1 blue, 2 yellow, 3 black, 4 white, 5 chequered, 6 penalty, 0 none (0 on the frame the manager is first looked up) |
| `idealLineOn` | 0x110 | `sim->drivingAidsManager->idealLine` |
| `isInPitLane` | 0x114 | `CarAvatar::isInPitlane` 0x1400d8c30 |
| `surfaceGrip` | 0x118 | `sim->physicsAvatar->engine.track->dynamicGripLevel` (live read of the physics track) |
| `mandatoryPitDone` | 0x11c | `avatar->mandatoryPitstopDone` (+0x1295; the pseudo-C mislabels it `wasInPitlane`) |
| `windSpeed` | 0x120 | `physicsAvatar->windData.speed.value * 3.6` |
| `windDirection` | 0x124 | `physicsAvatar->windData.directionDEG` |
| `timeLimitSessionLeft` | 0x128 | `sim->timeLimitedTest->lastRemainingTime`, or −1 |
| `isEscMenuVisible` | 0x12c | flag of `sim->escMenu` |

### 5.8 Static page (written once)

| Field | Offset | Source |
|---|---|---|
| `smVersion` | 0x000 | literal `"1.7"` |
| `acVersion` | 0x01e | from `changelog.txt` |
| `numberOfSessions` | 0x03c | `RaceManager::getSessionCount` |
| `numCars` | 0x040 | `sim->cars.size()` |
| `carModel`, `track`, `playerName`, `playerSurname`, `playerNick` | 0x044, 0x086, 0x0c8, 0x10a, 0x14c | strings (the track name comes from `Sim::getTrackData`; the other sources were only skimmed). Copy rule as for the lap strings: a fixed literal replaces a string that does not fit |
| `sectorCount` | 0x190 | `sim->track->physicsTrack.sectorsNormalizedPositions.size()` |
| `maxTorque`, `maxPower` | 0x194, 0x198 | `avatar->physicsInfo.maxTorqueNM`, `.maxPowerW` |
| `maxRpm` | 0x19c | `physicsInfo.maxRpm` (= `acEngine.defaultEngineLimiter`) |
| `maxFuel` | 0x1a0 | `(float) physicsInfo.maxFuel` |
| `suspensionMaxTravel[4]` | 0x1a4 | `physicsInfo.bumpStopsUp[i] − physicsInfo.bumpStopsDn[i]`; 0 for a wheel that is not double wishbone (both values are left 0, see `suspensionTravel` in 5.3) |
| `tyreRadius[4]` | 0x1b4 | `car->tyres[i].data.radius` |
| `maxTurboBoost` | 0x1c4 | `physicsInfo.maxTurboBoost` |
| `deprecated_1`, `deprecated_2` | 0x1c8, 0x1cc | left 0 |
| `penaltiesEnabled` | 0x1d0 | `raceManager->penaltiesEnabled` |
| `aidFuelRate`, `aidTireRate`, `aidMechanicalDamage`, `aidAllowTyreBlankets` | 0x1d4 … 0x1e0 | `PhysicsEngine::fuelConsumptionRate`, `tyreConsumptionRate`, `mechanicalDamageRate`, `allowTyreBlankets` |
| `aidStability` | 0x1e4 | `DrivingAssistManager::getStabilityControl` |
| `aidAutoClutch` | 0x1e8 | `avatar->physics->autoClutch.useAutoOnChange` |
| `aidAutoBlip` | 0x1ec | `CarAvatar::getAutoBlip` |
| `hasDRS`, `hasERS`, `hasKERS` | 0x1f0, 0x1f4, 0x1f8 | `physicsInfo` flags |
| `kersMaxJ`, `ersMaxJ` | 0x1fc, 0x250 | `physicsInfo.kersMaxJ`, `.ersMaxJ` |
| `engineBrakeSettingsCount`, `ersPowerControllerCount` | 0x200, 0x204 | `physicsInfo` |
| `trackSPlineLength` | 0x208 | `Spline::length` of `TrackAvatar::getAISpline`, 0 when there is none |
| `trackConfiguration` | 0x20c | track layout name |
| `isTimedRace`, `hasExtraLap` | 0x254, 0x258 | only when `sim->client != null` (online): any session with `isTimedRace`; `RaceManager::hasExtraLap`. Offline: 0 |
| `carSkin` | 0x25c | `CarAvatar::getCurrentSkin` |
| `reversedGridPositions`, `PitWindowStart`, `PitWindowEnd` | 0x2a0, 0x2a4, 0x2a8 | only when `sim->client != null` (online): `RaceManager::getInvertedGridPositions`, `getPitWindowStart`, `getPitWindowEnd`. Offline: 0 |

### 5.9 `Car::getPhysicsState`: which physics member each snapshot member copies

Runs on the physics thread after every step, under the `PhysicsAvatar` mutex, for every car.
The main thread copies the vector once per frame (`PhysicsAvatar::update`, which also
extrapolates the matrices by the time since the step when matrix smoothing is on) and
`CarAvatar::setNewPhysicsState` stores it in `CarAvatar::physicsState`. Only the members marked
**(page)** reach the shared memory.

| `CarPhysicsState` member | Copied from |
|---|---|
| `rideHeight[2]` **(page)** | `Car::computeRideHeight(i, &groundPlane)` or `RaceEngineer::evalFront/RearRideHeight` (section 5.3). The speed test reads the snapshot's own `speed`, i.e. the value of the previous call |
| `limiterRPM` **(page)** | `acEngine` virtual +0x08 = `Engine::getLimiterRPM` |
| `kersIsCharging` **(page)** | `ers.isCharging` (ERS cars only) |
| `p2pStatus`, `p2pActivations` **(page)** | from `acEngine.p2p`: 3 active; else 2 when `activations ≥ 1` and `timeAccum > coolDownS`; else 1; activations byte |
| `lapTime`, `lastLap`, `bestLap` **(graphics page)**, `lapCount` | `transponder.t`, `.lastLap`, `.bestLap`, `.lapCount` |
| `worldMatrix` | `body->getWorldMatrix` |
| `suspensionMatrix[i]`, `tyreMatrix[i]` | `suspensions[i]->getHubWorldMatrix` (+0x08); tyre = `Tyre::getFinalTyreRotation` with the hub position |
| `engineRPM` | `max(Drivetrain::getEngineRPM(), 0)` |
| `isEngineLimiterOn` | `acEngine` virtual +0x10 = `Engine::isLimiterOn` |
| `wheelAngularSpeed[i]` | `T[i].status.angularVelocity`, or 0 when locked or sleeping |
| `steer`, `gas`, `brake`, `clutch`, `gear` | `controls.steer * steerLock`, `controls.gas`, `.brake`, `.clutch`, `drivetrain.currentGear` |
| `speed` | `Car::getSpeed()` (cached) |
| `velocity`, `localVelocity`, `angularVelocity`, `localAngularVelocity` | body slots +0x78, +0xb8, +0xa8, +0xb0 |
| `slipAngle[i]` | `T[i].status.slipAngleRAD * 57.29578` (**degrees**) |
| `slipRatio[i]`, `tyreSlip[i]`, `ndSlip[i]`, `load[i]`, `Dy[i]`, `Mz[i]`, `tyreDirtyLevel[i]` | `status.slipRatio`, `.slipFactor`, `.ndSlip`, `.load`, `.Dy`, `.Mz`, `.dirtyLevel` |
| `tyreSurfaceDef[i]` | copy of `*T[i].surfaceDef` when not null |
| `cgHeight`, `accG` | as in section 5.3 |
| `lastFF_Pure`, `lastFF_Final` | `lastFF`; `getFFGlobalGain() * userFFGain * lastFF` |
| `aero` | `aeroMap.dynamicCD`, `dynamicCL` (twice) |
| `tyreContactPoint[i]`, `tyreContactNormal[i]` | `T[i].contactPoint`, `.contactNormal` |
| `camberRAD[i]` | `status.camberRAD`, negated for odd `i` |
| `tyreRadius[i]`, `tyreLoadedRadius[i]` | `T[i].data.radius`; `radius − status.depth` |
| `suspensionTravel[i]` | `suspensions[i]->getStatus().travel` |
| `normalizedSplinePosition` | `splineLocatorData.npos` |
| `driftPoints`, `instantDrift`, `isDriftValid`, `driftComboCounter`, `driftBonusOn` | `driftMode` members |
| `drivetrainSpeed`, `turboBoost`, `turboBov`, `turboBoostLevel`, `engineLifeLeft`, `isGearGrinding`, `gearRpmWindow` | `|Drivetrain::getDrivetrainSpeed()|`, `acEngine.status.turboBoost`, `acEngine.bov`, `Engine::getTurboBoostLevel()`, `acEngine.lifeLeft`, `drivetrain.isGearGrinding`, `Drivetrain::getRpmWindowStatus()` |
| `performanceMeter`, `performanceMeterSpeedDiffMS` | `PerformanceMeter::getCurrentSplit` `.t`, `.speedMS` |
| `bodyWorkVolume` | Σ over wheels of `|(bodyVelocity − hubVelocity) · body y axis|` |
| `tyreVirtualKM[i]`, `tyreGrain[i]`, `tyreBlister[i]`, `tyreFlatSpot[i]`, `tyreInflation[i]`, `wear[i]`, `wearMult[i]` | `status.virtualKM`, `.grain`, `.blister`, `.flatSpot`, `.inflation`, `min(virtualKM / modelData.maxWearKM, 1)` (−1 when `maxWearKM == 0`), `.wearMult` (written only when `maxWearKM != 0`; otherwise the old value stays) |
| `susDamage[i]`, `discTemps[i]`, `tyreThermalStates[i]` | `suspensions[i]->getDamage()` (+0x88), `brakeSystem.discs[i].t`, `Car::getTyreThermalState` |
| `damageZoneLevel[5]`, `groundPlane`, `timeStamp`, `airDensity`, `fuel`, `fuelLaps`, `water` | `damageZoneLevel`, plane through `tyres[0..2].unmodifiedContactPoint`, `ksPhysics->physicsTime`, `aeroMap.airDensity`, `(float)fuel`, `FuelLapEvaluator::getFuelLaps`, `water.t` |
| `kersCharge`, `kersInput`, `kersCurrentKJ` | ERS members when `ers.present`, otherwise the KERS members. Unlike the page, the snapshot does not test `kers.present`: a car with neither system gets the (idle) `Kers` values, the page gets 0 |
| `isRetired`, `setupState`, `lockControlsTime`, `physicsGUID`, `statusBytes`, `actionsState` | `isRetired`, `setupManager.setupState`, `lockControlsTime − physicsTime`, `physicsGUID`, `statusBytes` masks 0x1 lights on / 0x2 DRS available / 0x4 DRS active (the two DRS masks only when `drs.isPresent`), button bits from the controls provider and `controls` |
| `antiSquat`, `caster[2]` | player car only: `RaceEngineer::getAntiSquat`, `getCasterRAD * 57.29578` |

---

## 6. Connections to other systems

This system only reads. It runs **after** the whole `Car::stepComponents` list, after the ODE
step and after `Car::postStep`, so every component has finished its step when the page is filled.

| Reads | Written by (where in the step) |
|---|---|
| `Car::controls` | `Car::pollControls` at the top of `Car::step`; then overwritten in places by `Car::step` (forced stop), `Autoclutch::step` / `stepSequence` (`clutch`), `AutoBlip::step` and `AutoShifter::step` (`gas`). Found by searching the pseudo-C for stores to `controls.gas/brake/steer/clutch` |
| `Car::accG`, `Car::lastVelocity` | `Car::step`, just before `stepComponents` |
| `Car::valueCache.speed` | `Car::stepPreCacheValues`, before `Car::step` |
| `Tyre::status`, `contactPoint`, `contactNormal`, `roadHeading`, `thermalModel` | `Tyre::step` (4th item of `stepComponents`) |
| `ISuspension::status.travel` | each suspension's `step` (3rd item) |
| `BrakeSystem::discs[i].t`, `frontBias` | `BrakeSystem::step` (1st item) |
| `Drivetrain::currentGear`, `engine.velocity`, `acEngine.status.turboBoost` | `Drivetrain::step` and what it calls |
| `DRS`, `Kers`, `ERS`, `ABS`, `TractionControl`, `SpeedLimiter`, `AutoShifter` flags | their own `step` in `stepComponents` |
| `LapInvalidator::currentTyresOut`, `PerformanceMeter` | player-only part of `stepComponents` |
| `Car::lastFF` | `Car::onTyresStepCompleted` (force feedback) |
| `Car::splineLocatorData` | `Car::postStep` |
| `Car::body` velocity, matrix, position | ODE (`PhysicsCore::step`), see section 5.5 for the timing |
| `Car::transponder` | `TimeTransponder::step` |
| `CarAvatar::physicsState` | main thread, `PhysicsAvatar::update` → `CarAvatar::setNewPhysicsState` |
| `CarAvatar` cockpit settings | main thread UI code |

Outputs: the three pages, read only by other programs (our logger, dash apps). The older page
`Local\acpmf` and the UDP server are parallel exports that read `CarAvatar::physicsState` on the
main thread; nothing in the game reads any of them back.

Thread-safety facts that matter to a reader: the physics page is consistent in itself except for
the two snapshot values; the graphics page mixes main-thread values with three unprotected live
reads of the physics car (`npos`, body position, track grip).

---

## 7. Port notes

**Effort: S.** The link itself is one struct layout and about sixty assignments with a handful of
conversions; there is no state except two counters. It has no value before a whole car exists, so
it belongs at the end of the port order, right after the car step.

**Port before it:** everything the page reads: `Car::step` with controls, tyres (done),
suspension, drivetrain, brakes, aero (air density), the rigid-body core (velocity, matrix), and
the small `Car` getters of section 3. For the graphics fields: `TimeTransponder` and
`SplineLocator` (track AI line).

**What to build in Rust:** a function `physics_page(&Car) -> SPageFilePhysics` (plain `#[repr(C)]`
struct, 4-byte packing, 592 bytes) that follows section 5.3 line by line. Keep these details:
`rpms` truncates and may be negative; `speedKmh` uses the cached speed and the float constant 3.6;
`tyreWear` converts `virtualKM` to float before the curve lookup; `camberRAD` is not sign-flipped;
`suspensionTravel` subtracts `bumpStopDn`; `kersCurrentKJ` multiplies by the float 0.001;
`heading` / `pitch` / `roll` use `atan2f` / `asinf` from `MSVCR120.dll` (the tyre port already
borrows that library's functions, so the same approach gives the same bits).

**Bit-exact test with an oracle (in-process `acs.exe`, like the tyre oracle):**

- `Car::getPhysicsState` 0x140270d70 takes `(Car*, CarPhysicsState*)` and nothing else. Given a
  real `Car` object inside the oracle it can be called directly on a 0xb70-byte buffer. It needs
  the car's `controlsProvider` (slots +0x10 and +0x28 are called), `ksPhysics` (`physicsTime`),
  `body`, the four suspensions, and it constructs a temporary `RaceEngineer`. So it is usable only
  once the "whole car in the oracle" problem of `docs/map/car_step.md` is solved; it cannot be
  tested on fakes cheaply.
- `SharedMemoryWriter::updatePhysics` 0x140186ef0 is easier to borrow than it looks. It never
  touches the Windows mapping API. It only needs a 0x220-byte block laid out like
  `SharedMemoryWriter` with: `car` (+0x60) = the oracle's `Car`; `avatar` (+0x68) = a zeroed
  buffer of at least 0x12a0 bytes (only `physicsState.rideHeight` +0xa04, `limiterRPM` +0x9d8 and
  the cockpit values are read; the ERS and P2P reads are skipped when the car has neither);
  `lapInvalidator` (+0x70) = null (the code tests it); `sharedMemories[2].mapFileBuffer` (+0xb8)
  = any 592-byte buffer; `sharedMemories[2].nullCounts` (+0xc4) = 300; `physicsInfo.bumpStopsDn` (+0x140) = the
  four `ISuspension::bumpStopDn` values (0 for a wheel that is not double wishbone, as the game does). Call it with `rcx` = that block and `rdx` = a pointer to a
  double. The output buffer then holds exactly the bytes a logger would have read.
  One trap: it does `dynamic_cast<AIDriver*>` on the car's controls provider and calls its slot
  +0x28, so the provider must be an object whose vtable has valid RTTI in front of it (a fake
  vtable needs a copied "complete object locator" pointer, or use a real provider class).
- The comparison then is `physics_page(rust_car)` against that buffer, byte for byte, after each
  oracle step. This gives a per-step check of the whole car with the same columns as the recorded
  lap, without the 5-decimal rounding.
  Do this comparison with the oracle's own timing: read from the code, the oracle's page pairs the
  post-integration body state with the pre-integration `speedKmh` / `accG` (section 5.5), unlike
  the recorded lap. Fill the Rust row the same way the oracle does, whichever it turns out to be.
- The small getters can be checked alone on fakes: `Drivetrain::getEngineRPM`,
  `BrakeSystem::getFrontBias`, `Car::computeRideHeight` (needs a body matrix and a plane),
  the angle formulas (pure float maths on a matrix).

---

## 8. Open questions

1. **Timing of the body values (section 5.5).** The call order read from the pseudo-C
   (`stepPreCacheValues` → `Car::step` → `PhysicsCore::step` → writer) predicts that `velocity` is
   one step newer than `speedKmh` and `accG`. The recorded lap shows them to be the same step, and
   shows `velocity = 0` while the cached speed is not zero in the sleeping state. Either the ODE
   velocity update is not visible yet when the writer runs, or one of the links in that call chain
   is not what it seems. I could not find the mechanism in the code. I chose to document the
   behaviour the data shows and to flag it. An oracle run settles it in minutes: step a car once
   and read `dBodyGetLinearVel` before and after `PhysicsCore::step`. This also matters to
   `car_step.md` and `physics_engine.md`.
   Review pass: the call chain was re-read end to end and holds, and the data facts were
   reproduced (section 5.5). The remaining candidate is the third-party patch that was loaded when
   the lap was recorded (question 13). **Unverified either way**; until the oracle run is done,
   treat the 5.5 rule as a property of this recording, not of `acs.exe`.
2. **`fuel` unit.** Taken to be litres because the F2004 starts at 80.0 and the value is the
   `Car::fuel` double next to a separate `Car::fuelKG`. Not verified against car.ini here.
   In the lap the value rises from 80.0 to 82.23 at t = 4.07 s, before the launch (rows 1200 to
   1320 also show gear 0). Something sets the fuel during the start procedure; not traced.
3. **`carDamage` zone order** (front, rear, left, right, centre) is the public description of the
   interface, not checked in `Car::onCollisionCallBack`.
4. **`status` pause flag.** Resolved in review: +0x150 is `ksgui::Control::visible` (PDB layout),
   for both `sim->pauseMenu` (`status`) and `sim->escMenu` (`isEscMenuVisible`).
5. **Axis signs.** +z = forward and y = up are confirmed from the lap (heading against the
   velocity direction, braking gives negative `accG_z`). Which way local +x points (left or right)
   was not determined; check `accG_x` against a known corner before relying on its sign.
6. **`acVersion`** parsing of `changelog.txt`, the exact fixed literals used for over-long strings
   and the sources of the name strings of the static page were not followed (not needed for the
   lap check).
7. **Ghidra member names in `CarAvatar` above +0x1278 are shifted by 8 bytes** in the pseudo-C
   (it prints `currentERSPowerIndex` for +0x127c and `wasInPitlane` for +0x1295). The names in this
   document come from the PDB layout in `re/types/CarAvatar.txt` and the offsets in the
   disassembly. Other documents quoting those names from pseudo-C should be re-checked.
8. **`hotlapMode`** is set from `race.ini` but never read (the member name occurs only in the
   constructor across all of `re/decomp`). Dead state in this build.
9. **`TyreThermalModel::getIMO`** was not worked through (it averages the surface patches into
   inner / middle / outer); only its output order is used here.
10. **`wheelsPressure` unit (psi)** and **`turboBoost` unit (bar)** follow the tyre map and the
    public interface description; no conversion happens in the writer.
11. **`SharedFileOut::update` and `RemoteTelemetryUDP`** were only skimmed (constructor facts are
    confirmed, field mappings not listed).
12. **Sleeping state.** `Car::getPhysicsState` zeroes `wheelAngularSpeed` when
    `sleepingFrames > framesToSleep`; the physics page does not. Whether other page fields behave
    specially while the car sleeps was not checked beyond `velocity` and `accG`.
13. **The reference lap was very probably recorded from a patched game** (found in review; details
    and evidence in section 9.5). What the patch changes besides the air density is unknown: it is
    a 139 MB binary outside the scope of this map. Open: (a) does it explain the packet timing of
    section 5.5, (b) which other physics values of the lap differ from what `acs.exe` alone
    computes, (c) the stale first row (a mapping survives a session only while some other program
    keeps it open; a launcher that reads the shared memory would do that). The clean fix is a new
    recording from the unpatched game.
14. **Handler order in `car_step.md`.** That document lists `ReplayManager::onStepCompleted` before
    `SharedMemoryWriter::updatePhysics` on `PhysicsAvatar::evOnStepCompleted`. Read from
    `Sim::Sim` (the writer is constructed first, `addHandler` is a `push_back`) it is the other
    way round. Harmless for the numbers (the replay recorder does not change the car), but the
    other document was not edited from here.

---

## 9. Using this to check a Rust car against f2004_spa_ai.csv

### 9.1 What the file is

- **Read section 9.5 first**: the lap was very probably recorded with a third-party patch loaded,
  and at least `airDensity` is not the value the game code computes.
- 117,333 rows, 86 columns. Row 0 is a **stale page of the previous session** (packetId 1790):
  drop it. Rows 1 … 117,332 are packetId 1 … 117,410 of one session.
- One row = one physics step (the logger writes a row only when `packetId` changed). 78 steps are
  missing in 38 gaps; `packetId` tells exactly where. Use `packetId`, not row number and not `t`,
  as the step index: step time = `packetId * 0.003 s` from the first page.
- `t` is the logger's own wall clock and jitters by a millisecond; do not use it for dynamics.
- Content: the car stands for about 4.6 s (neutral, engine revved, body "sleeping"), launches at
  about packetId 1516 from track position 0.9379, crosses the line, then does three flying laps
  (111.408 s, 111.134 s, 110.999 s). `lap` goes 0 → 1 at packetId 43368 and 1 → 2 at 80408.
- The eight leading columns `lap`, `lapTimeMs`, `lastLapMs`, `trackPos`, `posX`, `posY`, `posZ`
  come from the graphics page: they update about 47 times a second and are repeated in about
  7 rows. `rideHeight_0/1` behave the same way although they sit in the physics page.
- Rounding by the logger: physics floats to 5 decimals, positions to 4, `trackPos` to 6. Large
  values (loads around 4000 N, rpm, wheel speed) keep all their float digits; small ones
  (slip, camber, angles, g) lose 2 to 3 significant digits. **No column is bit-exact.** Compare
  with an absolute tolerance of 5e-6 (5e-5 for the positions), plus real model error.

### 9.2 Columns by how useful they are

**Direct state of one physics object** (tight comparison, one object to blame when it differs):

| Columns | Object |
|---|---|
| `wheelLoad_*`, `wheelSlip_*` (ndSlip), `wheelAngularSpeed_*`, `camberRAD_*`, `tyreDirtyLevel_*`, `wheelsPressure_*`, `tyreCoreTemperature_*` | `Tyre` (already ported): the best first targets |
| `suspensionTravel_*` | one suspension each (minus a constant) |
| `gear`, `rpms` (integer part), `turboBoost` | `Drivetrain` / `Engine` |
| `velocity_*`, `localAngularVel_*`, `heading`, `pitch`, `roll` | the car body in the rigid-body core |
| `gas`, `brake`, `steerAngle` | `Car::controls` (inputs, see 9.3) |
| `fuel` | `Car::fuel` |
| `airTemp`, `roadTemp`, `ballast` | constants of the session: use them as inputs |
| `airDensity` | **not** a constant in this recording (1.1575 … 1.1717, it follows the car's height) and not what the game code computes (section 9.5). Feed the recorded value into the Rust aero step as an input; do not compare it with the Rust formula |
| `drs`, `tc`, `abs`, `pitLimiterOn`, `autoShifterOn`, `kersCharge`, `kersInput` | switch states. In this lap: `drs`, `abs`, `kersCharge`, `turboBoost`, `ballast` are always 0; `tc` is 0 or 0.08 |

**Derived, but cheap and exact to reproduce:**

| Columns | Rule |
|---|---|
| `speedKmh` | `3.6 * |velocity|` of the same row (except while sleeping) |
| `accG_*` | `(velocity − previous velocity) / 0.003 / 9.806`, rotated into the car frame; only valid across consecutive packetIds |
| `tyreWear_*` | wear curve at `virtualKM`, times 100: needs the compound's `WEAR_CURVE` |
| `cgHeight` | body height minus mean contact height: mixes body and four tyres |
| `finalFF` | steering force feedback times two gains: needs the FF code and the gain of the AI provider |
| `rpms` | truncated: compare `floor` of the Rust value, not the float |

**Poor for step-level comparison:** `lap`, `lapTimeMs`, `lastLapMs`, `trackPos`, `posX/Y/Z`,
`rideHeight_*` (frame rate, up to one frame old, position possibly torn across two steps),
`numberOfTyresOut`, `carDamage_*` (rules, not dynamics). They are still good for coarse checks:
lap time, the driven line, fuel per lap.

### 9.3 Sampling-rate implications

- The physics rows are at the full 333 Hz, nearly without holes, so step-by-step comparison is
  possible wherever `packetId` is consecutive. Restart the comparison after each gap (38 places).
- Remember the timing rule of section 5.5: within a row the tyre and control values belong to the
  step that just ran, and the body values are the state that step started from. A Rust harness
  should therefore fill its row from (body state before the step, component state after the step).
- The position columns cannot drive a step-level check. Integrate `velocity` between graphics
  updates if a position is needed at every step.

### 9.4 Replaying the lap: what is there and what is missing

Available as controls, every step: `gas`, `brake`, `steerAngle` (normalised steer; multiply by the
car's steer lock for degrees), and `gear` (the engaged gear after the gearbox logic, not the
up/down button).

Missing for a deterministic replay:

1. **`clutch`** is in the page (0x16c) but outside the logger's prefix. Needed for the launch and
   for every shift. Also missing: handbrake, the gear-up / gear-down button edges (only the result
   is visible), KERS/DRS buttons (unused on this car).
2. **AI-only effects.** The lap was driven by `AIDriver`. Besides setting the controls it can
   change the car directly (the tyre map lists `Tyre::aiMult`; `AIDriver` has functions such as
   `stepSuperhuman`). Those are not in the page. See the AI part of the atlas before trusting an
   open-loop replay.
3. **Assists between the pedal and the car**: the logged `gas` is `Car::controls.gas` as it
   stands at the end of the step, so it already contains the auto-blip and the auto-shifter gas
   cut (they write `controls.gas`). Feeding it back in as the driver's pedal would apply those
   twice unless the Rust assists are switched off. Traction control does not touch
   `controls.gas`: it works through `acEngine.electronicOverride` (seen in
   `TractionControl::step`), so its cuts are invisible in the log (only `tc`'s limit is there).
   The speed limiter (`SpeedLimiter::step` 0x1402bb910) does not touch `controls` either: in the
   pit lane above 80 km/h it sets `acEngine.electronicOverride = 0`, above 81 km/h also
   `brakeSystem.electronicOverride`. `Autoclutch::step` / `stepSequence` write `controls.clutch`
   (not logged).
4. **Track surface.** The lap needs the Spa collision mesh, `surfaces.ini` and the ray casts.
   The contact point, normal and heading of each tyre are in the page (0x1a4 … 0x233) but not in
   the logger's prefix; with them a replay would not need the mesh at all for the tyres.
   `surfaceGrip` (dynamic track grip, graphics page 0x118) and the wind are not logged either.
5. **Start state.** The first usable row is about 300 steps after the car was attached, with the
   car at rest. Known from it: position (4 decimals, frame rate), heading / pitch / roll, fuel, tyre core
   temperatures and pressures, wheel speeds. Unknown: hub positions and velocities, the full tyre
   thermal state (surface patches), brake temperatures, the exact body position at step
   resolution. A replay must settle the car on the ground first and accept a small offset.
6. **Open-loop drift.** Even with perfect inputs, a replay of steering against time diverges within
   seconds on a real track. Two workable uses instead: (a) *forced-state checks*: at each row set
   the Rust body velocity, wheel speeds and controls from the row, run one step, and compare the
   outputs of the next row (loads, slip, travel, `accG`); (b) *closed loop*: let a simple driver
   follow the recorded line (`pos*`, `trackPos`) and speed, and compare lap time, speed trace and
   fuel per lap.

### 9.5 Warning: this lap does not come from the unpatched game

Found in the review pass. Facts (each checked, read-only):

- The game folder holds `dwrite.dll` (139 MB, not a system DLL of that size) and an `extension/`
  folder whose `installed.log` starts with "Generated automatically during last patch installation
  via Content Manager". That is a third-party patch loader that hooks `acs.exe` at run time.
  `installed.log` is dated 2026-10-05 00:16; `f2004_spa_ai.csv` was written 2026-10-05 13:06,
  after it.
- `airDensity` in the lap is not what `acs.exe` computes. The code (`Car::updateAirPressure`
  0x140276ae0 → `PhysicsEngine::getAirDensity`) gives `1.2922 − 0.0041 * 14 = 1.2348` for the
  recorded `airTemp` of 14 °C, constant, and only ever lower through another car's slipstream.
  The lap reads 1.16714 while the car stands still and then follows the car's height:
  `ln(airDensity)` against `posY` is a straight line (r² = 0.9987, 1,423 distinct values) with a
  slope of −1 / 8418 m, the barometric scale height. No function of `acs.exe` makes the density
  depend on height.

Interpretation (mine, not proven): the patch was active and replaces at least the air-density
model; it may also be what moves the page write relative to the rigid-body step (section 5.5).

What it means for the comparison:

- The page reads the very member (`aeroMap.airDensity`) that the wing and drag code multiply by
  (`aero.md`), so aero forces in the lap are computed with a density about 5.5 % below the
  vanilla value, varying by 1.2 % around the lap. A Rust car that follows `acs.exe` exactly will **not** match this
  lap's top speed, loads or lap time unless `airDensity` is taken from the CSV row.
- Any other column may carry patch effects that this document cannot list. Agreement with the lap
  is evidence, disagreement is not proof of a Rust bug. The bit-level reference is the in-process
  oracle of the unpatched `acs.exe` (section 7), not this file.
- A new recording made with the patch disabled would remove the doubt. Check first that
  `airDensity` then reads the constant `1.2922 − 0.0041 * airTemp`.

Cheapest improvement for the next recording: extend the logger's `Physics` struct to the full
592 bytes (so `clutch`, `brakeTemp`, `tyreTempI/M/O`, `tyreContactPoint/Normal/Heading`,
`brakeBias`, `localVelocity` are captured), read the page twice per row to reject torn copies, and
stop rounding (store the raw float bits).
