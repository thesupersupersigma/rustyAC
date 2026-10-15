# Physics extensions: what CSP hooks in the car core and the physics tick

CSP 0.3.0-preview649 (build 4184) does not add a layer on top of acs.exe's car physics: it replaces the physics core (the rigid-body library), the physics thread loop and `PhysicsEngine::step`, and it wraps or re-implements `Car::step`, `Car::stepComponents` and about twenty small `Car::*` functions. Most of the wrapping is switched by one per-car object ("the extended state", table `0x26ae290` in the dll, see section 2a): a car without that object runs the original acs.exe function; a car with it gets extra work before and after, and a car-physics Lua script is called from `Car::pollControls`. This file maps the 58 hooks and 26 lookups of the car-core classes and builds the 3 ms tick with the CSP injection points marked.

All dwrite.dll addresses are RVAs (image base `0x180000000`); acs.exe addresses are the `0x14...` VAs of `docs/map`. Evidence markers: [dll] seen in the decompile / tables, [log-csp], [ini], [sdk], [prior], [interp] = my inference.

## 1. What it does

What the owner sees / what changes in the simulation, in order of importance:

1. **A different physics core.** `ksPhysicsCoreODEFactory::create` (acs 0x1402cb630) is fully replaced (hook `0x6216c0`, category C): it allocates a 0x32c0-byte CSP core object (RTTI `cphys::ode64_core`, `ode_impl_core`, double precision) and publishes it in two globals. Everything that acs.exe asks of `IPhysicsCore` (bodies, joints, rays, colliders) is then answered by CSP's ODE build ("ODE flags: ... ODE_EXT_libccd ... ODE_EXT_mt_collisions ODE_EXT_threading ODE_double_precision" [log-csp]; tyre rays through Intel Embree: "Starting Intel Embree build" [log-csp]). `PhysicsEngine::onCollisionCallBack` (acs 0x140264020) is replaced by an empty function (39 symbols share that one `ret` stub, among them `SharedMemoryWriter::updatePhysics`, `Tyre::~Tyre`, `PhysicsAvatar::~PhysicsAvatar`), so acs.exe's collision-damage path is only reached if CSP calls the `Car::onCollisionCallBack` lookup itself [interp].
2. **A different physics thread.** `PhysicsDriveThread::run`, both registered `PhysicsDriveThread::step` hooks, `PhysicsEngine::step`, `PhysicsAvatar::startPhysics/shutdown/render` are replaced (C). The step is still 3 ms (the constant `0.003f` and `3.000000026` ms are in the code), but the clock logic, catch-up rule, car loop (CSP's own LLTP worker pool, "CSP uses 7 workers" [log-csp]), optional "collisions in parallel with cars" (`PARALLELIZED_CONTACTS`), and a per-section profiler are CSP's. The game does not wait for the old `ThreadPool`.
3. **The car step order is kept, but every part is wrapped.** `Car::stepComponents` is re-implemented (C, original never called) in exactly acs.exe's order (29 components), with profiling scopes ("Car #1/Components", "Car #1/Tyres", "Car #1/Suspensions", "Car #1/Aero", "Car #1/Traction control" [dll strings]) and, for cars that have the extended state, extra pre/post work between the components. `Car::step` is a wrapper (B) around the original.
4. **Per-car Lua physics script.** For cars that have one (`data/script.lua`, SDK library `ac_car_cphys`) the script's update runs on the physics thread from `Car::pollControls` (hook `0x59fe60`), each 3 ms step, after the original poll. See 2c.
5. **Always-on small fixes** that apply to stock cars too (prior `csp_survey.md` P1, P7, P8, P10, P22): air-density law with altitude, 1.5 m pit test, smooth controls locking, precise (1 ms) transponder, adaptive performance meter, mesh collider "sooner" and clamped to the box colliders, optimized track-object colliders, per-wheel "awake in pits", pit stop time composition from the server rule `PITS_ORDER`, server `[EXTRA_RULES]` (pit limiter speed, lock-controls time caps, slipstream multipliers).
6. **Online/server rules** (`_DAT_1826a9690`, filled by function `0x945a70` from `[EXTRA_RULES]`) change the pit limiter speed, `lockControlsUntil`, slipstream gain, pit-stop composition, penalty reasons.

## 2. Game parts it replaces or extends

Categories: (A) pass-through that only records state / fires CSP events, (B) wraps the original and changes inputs or outputs, (C) fully replaces (original never or only conditionally called), (D) only reads/forwards (a lookup: CSP calls the acs.exe function through its slot). "slot" = dwrite.dll RVA of the data slot that holds the trampoline to the original (the replaced code calls `[slot]`). `ext` = the per-car extended state (2a).

Totals for the classes of this file: **58 hooks, 26 lookups** (Car 26+7, CarColliderManager 1+1, PhysicsObject 2+0, PhysicsEngine 4+5, PhysicsDriveThread 4+0, PhysicsAvatar 8+0, DynamicController 3+1, SpeedLimiter 2+0, Telemetry 0+1, AutoBlip 0+1, AutoShifter 0+1, DriftModeComponent 0+1, EDL 0+1, StabilityControl 0+1, SteeringSystem 0+1, TimeTransponder 5+3, PerformanceMeter 2+1, PenaltyManager 1+1), 231,056 bytes of decompiled pseudo-C (Car alone 138,023 bytes, all read). Plus 13 `Sim::` hooks of which 7 touch the physics clock (2g).

### 2.1 `Car` (acs 0x14026bf00..0x140276df0; 26 hooks, 7 lookups)

| Symbol | acs VA | CSP RVA (slot) | Cat | What it changes, and when |
|---|---|---|---|---|
| `Car::Car` | 0x14026bf00 | 0x595b40 -> body 0x595b70 (0x268aab8) | B | Thin entry; the 2,208-byte body calls the original constructor through the slot, around it reads `suspensions.ini` section keys `TYPE` of FRONT/REAR [dll immediates] (the stock exe exits on `TYPE=COSMIC`, docs/map/car.md 5.1.5; CSP handles type 5 = COSMIC itself) and afterwards attaches the CSP per-car object with 0x5a07e0 / 0x5a07c0. Every car. Body not decompiled (request). |
| `Car::step` | 0x140275da0 | 0x5996f0 (0x268aa98) | B | Wrapper, see the sequence in 2b step 3. Profiler scope per car; GUID 0 only: calls 0x599000; other cars: writes 250 into Car+0x3e28 (the sleep-frames limit, acs default 50) every step; with ext: pre-work (record body position, double-buffered 0x6c-byte snapshot, wheel steering geometry for car index 0), then the original, then post-work 0x597f00 and 0x10eb8f0. |
| `Car::stepComponents` | 0x1402764d0 | 0x599ec0 (0x268aa90, never called) | C | Re-implements all 29 calls in acs order (table 2b.2). The per-GUID-0 section (Telemetry, DriftModeComponent, PerformanceMeter, LapInvalidator, PenaltyManager) is kept (Car+0x3238 == 0). |
| `Car::pollControls` #1 | 0x140274e70 | 0x59fdb0 (0x26a47f0, never called) | C | Zeroes `CarControls` (Car+0x140..0x174), optionally injects steer from a signed 16-bit value at Car+0x3e8e when flag bit 0x20 of Car+0x3db4 is set, then **runs the Lua script of the car** (ext+0x250 holder) via the loader 0x1373420 and the updater 0x1375740, and copies the controls the script produced (gas, brake, steer, clutch, handbrake, 4 button bytes) plus per-wheel inputs into the car. Branch on `_DAT_1826add30 < 10` (see 2a). Looks like an alternative "script is the driver" implementation; see open question 1. |
| `Car::pollControls` #2 | 0x140274e70 | 0x59fe60 (0x26a47f8) | B | Shutting down (`DAT_18268b39b`): zero controls and return. Online and the client's +0x148 flag: `lockControlsTime = max(lockControlsTime, physicsTime + 5000 ms)`. Then **original**, then: flag 0x10000 -> brake = 1.0 (stationary auto-brake, `DISABLE_AUTOBRAKING` [ini]); flag 0x20 -> steer from the int16 at 0x3e8e; flag 0x40 -> swaps steer with a stored previous value (a one-step delay buffer); then the physics scripts of ext (+0x258, +0x250) are updated by 0x856ed0 (2c). |
| `Car::updateAirPressure` | 0x140276ae0 | 0x596f80 (0x268aaa0, never called) | C | Air density = ext's altitude-dependent law (function 0xaf7760 on ext+0x228, matches P1 -5.6 % downforce) or the acs formula `1.2922 - 0.0041 * T` when ext has no such data; times a per-car multiplier table (`0x25e1a30[GUID]`, the Lua `physics.setAirDensityMultiplier`) ; slipstream: same formula as acs but with the maximum effect over other cars, `SLIPSTREAM_GAIN_MULT` / `SLIPSTREAM_MULT` from `[EXTRA_RULES]`, and a smoothed copy (Car+0xaec, factor 0.015/step) for audio and visuals. Result in `aeroMap.airDensity` (Car+0xae8). |
| `Car::updateColliderStatus` | 0x140276df0 | 0x59c300 (0x26a47c8) | B | Original first. Then (a) flag bit 0x2 of Car+0x3db4: clears bits 0xc of the body collider's category mask; otherwise, when Car+0x3e68 is set and the `[EXTRA_RULES]` block has its byte +1 set, sets bits 0xc [dll]. (b) `unloaded` = any of the four per-tyre floats at Car+0xea0 / +0x16f8 / +0x1f50 / +0x27a8 is <= 0.0001 [interp: a wheel without load]. (c) With the option byte `0x269508a` (ENABLE_MESH_COLLIDER_SOONER by behaviour), ext present and flag 0x200 clear: if `unloaded`, sets a collider parameter (+0x128) to 1e-4 / 1.0; else keeps a 0..255 hysteresis counter in Car+0x3e2d from the four tyre bytes (+0x10ca, +0x1922, +0x217a, +0x29d2: +1 or +10 when two or more are above 10 / 30, decays otherwise) and sets the same parameter to max(clamp01((ext+0x4dc - 0.7) * -3.33), clamp01((counter - 30) / 90)). (d) Finally calls collider method +0x130(1, unloaded): the mesh collider (index 1) is enabled only while some wheel is unloaded. |
| `Car::initColliderMesh` | 0x140273b20 | 0x59ce20 (0x26a47d0) | B | Before the original: reads `colliders.ini` `[COLLIDER_n] CENTRE/SIZE` and `suspensions.ini [FRONT] BASEY` [dll immediates], finds the lowest box-collider point and clamps the mesh vertices so the mesh does not go below the boxes (`CLAMP_MESH_COLLIDERS`, global byte `0x26950 8b`); sets flag bit 0x200 of Car+0x3db4 when the span is under 0.3 (clamp skipped). With global byte `0x2695069` set it calls CSP's own initialiser 0x59c6e0 instead of the original ("Physics data is missing: %s", "Car bounds L: W: front: rear:"). Key fragments `MESH_OFFSET`, `..._EXTENSION` also read. |
| `Car::forcePosition` | 0x14026fe10 | 0x59bf00 (0x268aa88) | B | Called from the main thread: copies the arguments and queues the call to the physics thread (0x6831b0). On the physics thread: guard flag Car+0x3db4 bit 0x100, original, then CSP post: GUID 0 updates a "last teleport position" global; releases the CSP pit/repair joint objects of ext (+0x1f0, +0x220), resets script holders (+0x250, +0x258), sets the extended-state "needs re-sync" counter; stops the 4 suspension objects (+0x98); fires CSP event list `0x268ff58` with the car. |
| `Car::reset` | 0x1402758e0 | 0x59c0d0 (0x268aa80) | B | Original, then CSP event list `0x268ff50`, then resets a per-car smoothing object (record+0x88: 4 child objects get value 0 when its value differs more than 0.005). |
| `Car::getPhysicsState` #1 | 0x140270d70 | 0x59dfa0 (0x26a47d8) | B | Original, then overwrites the tyre part (function 0x10aed70 on the 4 tyre blocks +0xf10/+0x1768/+0x1fc0/+0x2818) and `state+0xb6c = Car+0x150`. If the global "preview state" byte `0x2699 58b` is set it replaces the whole 0xb70-byte state by a captured copy; otherwise, when option byte `FUN_180ef1700()+5` is set and `state[0]==0`, it loads a stored state file named from the car's unix name + `preview_state` and keeps it ("Unexpected end" on short files). A showroom / preview-mode feature [interp]. |
| `Car::getPhysicsState` #2 | 0x140270d70 | 0x59e920 (0x26a47e0) | B | Two paths. Normal: temporarily sets sleeping-frames (Car+0x3d94) to `framesToSleep+1` when flag 0x10000 and speed < 0.1 (so the exported "sleeping" state is true while stationary), calls the original, then overwrites the tyre part like #1, ORs `0x40` / `0x400` into `state+0x7d4` for flags 0x200000 / 0x20000, low-passes accG (3 floats, factor 0.25), low-passes the local angular velocity (factor 0.02, only when flag 0x4 and global `0x26951ca` clear), forces `state+0x25c = steerRatio * <value>` when flag 0x400000, lets the ext's two script holders annotate the state (0x858b30) when flag 0x80, scales a vector by `2*speed-1` when speed < 1, zeroes `state+0xb4c` online with the client flag, and writes an "invalid contact" pattern when any tyre byte is 0xff. Override path (when 0x59c1f0 returns an object): the state is taken from a ReplayRecorder-style provider (0x11a9cb0) with a blend that decays by 0.0003 per call, i.e. about 3,333 steps (11 s); used for externally-driven / restored cars [interp]. |
| `Car::getWingState` | 0x140272a20 | 0x59fff0 (0x26a4800, never called) | C | Same copy loop as acs (0x44-byte `WingState` per wing from `aeroMap` wing list, stride 0x310), angle field from the wing's override angle when dynamic controllers are active; a CSP setting (`0x25a2cd0`, -1 = unset) can force the output vector size; when 0x59c1f0 returns an override object it forwards to `ReplayRecorder::getWingState` (slot 0x268c850). |
| `Car::getSteerFF` | 0x140272180 | 0x595800 (0x268aad8) | B | Returns 0 if the global "FFB off" byte `FUN_180ef1700()+3` is set. Stores Car+0x3e34 in Car+0x3dd4. Only for car index 0 (Car+0x3e9c == 0): if flag 0x8000 it scales a per-front-suspension value by a per-car factor; temporarily writes `steerAssist` (Car+0x3c70) = 1.0 when global byte `0x26995 8a` is set; calls the original; then ext's FFB tweak object (ext+0x268) smooths / scales the value, a script-provided value (ext+0x250 -> +0x68 +0x1090 != FLT_MAX) replaces it (clamped to -1..1, forced 0 when the script says "no FFB"), then **CSP event list `0x268ff60`** (handlers get `{car, &ff}`; this is where `ffb_tweaks.ini` / FFB post-processing hook in [interp]), then NaN/Inf guard -> 0. |
| `Car::getCGHeight` | 0x1402703b0 | 0x595750 (0x268aac8) | C (cond.) | With ext: height of the body position (ext+0x418, stored each step in `Car::step`) over CSP's ground query (`RaceEngineer::getPointGroundHeight`, hook 0x10ebeb0). Without ext: original. |
| `Car::computeRideHeight` | 0x14026fc90 | 0x5972b0 (0x268aaa8) | C (cond.) | With ext: ride height at front (index 0) or rear (1) from the contact point offsets Car+0x3df4 + 12*i transformed by the hub body (suspension type 5 = COSMIC) or the car body, over the same ground query. Without ext: original. |
| `Car::getGroundWindVector` | 0x140270c00 | 0x597210 (0x268aab0) | B (A-like) | Original; with ext copies the returned vector into ext+0x434 for scripts. |
| `Car::getPitstopTime` | 0x140271e60 | 0x5a0120 (0x26a4808) | B | Original; then (global `0x268b14c` = server rule `CHANGE_TYRES_PER_AXIS` [interp]) doubles the tyre time unless a per-wheel-change flag; then rebuilds `out[0]` (total) from the server string `PITS_ORDER` (`_DAT_1826a9690+0x48`): letters F = fuel `out[3]`, R = repair `out[2]`, T = tyres `out[1]`; `<` ... `>` is a parallel group (maximum), outside groups add. Car index 0 also stores the three component times in globals (`0x268b430/434/438`) for the HUD. |
| `Car::isInPits` | 0x140274530 | 0x595590 (0x26a3fa0) | C (cond.) | With `EQUALIZE_PITS_CHECK` (global `0x268bcb2`): body within 1.5 m of the pit position (dist^2 < 2.25) instead of acs's 3 m (dist^2 <= 9). |
| `Car::isSleeping` | 0x1402745e0 | 0x595710 (0x268aad0, never called) | C | `sleepingFrames (0x3d94) > framesToSleep (0x3e28)`, plus: true when flag 0x10000 and speed (Car+0x3e90) < 0.1. |
| `Car::lockControlsUntil` | 0x140274600 | 0x59f4a0 (0x26a47e8) | B | `[EXTRA_RULES] LIMIT_LOCK_CONTROLS_TIME` caps the requested lock to `rule * 1000` ms before the original; afterwards `LIMIT_LOCK_CONTROLS_TOTAL_TIME` caps `lockControlsTime` to `now + rule*1000`. |
| `Car::addPenalty` | 0x14026f6a0 | 0x59dd40 (0x268aa70) | B | Sets a per-thread "reason code 4" in a CSP penalty record (0x6825b0) and calls the original. |
| `Car::setBlackFlag` | 0x1402759f0 | 0x59dd80 (0x268aa60) | B | Replaces flag reason 4 / 1 by the code stored in the CSP penalty record when it differs (so scripts / rules can name the reason), then original. |
| `Car::setControllerProvider` | 0x140275a70 | 0x595660 (0x26a3fa8) | B | If the current provider pointer equals one of three CSP-owned provider objects (`0x26a4d98`, `0x26a4d78`, `0x26aaf10`) it is nulled first so the original does not delete them. |
| `Car::setDamageLevel` | 0x140275b20 | 0x59c1b0 (0x26a47c0, never called) | C (gated) | Writes the value into the 5 damage-zone floats (Car+0x3de0..0x3df4) unless flag 0x1000000 (damage locked) is set. |
| `Car::resetSuspensionDamageLevel` | 0x140275970 | 0x5956a0 (0x268aae0) | B | Original (unless flag 0x1000000), then sets ext's repair bits (`ext+0x6d1 = 0xf`), which `stepComponents` consumes (2b.2 item 13). |
| lookups | | | D | `Car::forceRotation` 0x140270040 (0x268aa78), `Car::getTotalMass` 0x140272570 (0x268aaf0), `Car::isInPitLane` 0x1402744e0 (0x268aaf8), `Car::onCollisionCallBack` 0x140274650 (0x268aac0), `Car::onTyresStepCompleted` 0x140274cd0 (0x268aae8, called by CSP's `stepComponents` itself), `Car::setRestrictor` 0x140275d10 (0x268aa68), `Car::stepPreCacheValues` 0x1402768c0 (0x268aa58). |

Car fields that the hooks touch (offsets from the hooks; names from docs/map where known): controls `gas/brake/steer/clutch` at +0x164/+0x168/+0x16c/+0x170 (docs/map/telemetry.md, steering.md); `steerLock` +0x174, `steerRatio` +0x178; drivetrain +0x310; BrakeSystem +0x2ca0; `physicsGUID` **+0x3238** (confirmed: it gates exactly the acs "GUID 0 only" list); `controlsProvider` +0x3d80; flags dword +0x3db4 (not in docs; CSP-side meaning in 2a); `sleepingFrames` +0x3d94, `framesToSleep` +0x3e28 (so `isSleeping` reads "sleepingFrames > framesToSleep"); speed cache +0x3e90; **+0x3e9c = a second car index** (see 2a).

### 2.2 `PhysicsEngine`, `PhysicsDriveThread`, `PhysicsAvatar`

| Symbol | acs VA | CSP RVA (slot) | Cat | What it changes, and when |
|---|---|---|---|---|
| `PhysicsEngine::PhysicsEngine` | 0x140262430 | 0x68aa80 (0x268b408) | B | Stores `this` in the global engine pointer `_DAT_1826a6de0` (read by every other hook), calls the original, then initialises CSP's engine-side data from `this+8` (cars vector) with 0x68b600. |
| `PhysicsEngine::step` | 0x140264760 | 0x68b580 (0x26a6e00) | C | acs's per-step body replaced: 0x68abe0 (41 B: stamps time / counter), 0x68acd0 ("Preparing / Computing cars", calls the `Car::step` hook; the only decompiled caller list is 0x689170, 0x689628, 0x68b580), then `core->step(0.003)` (vtable +0x10 of engine+0x190), then the evOnStepCompleted handler loop (engine+0x70..0x78, 0x28 each, argument = pointer to physicsTime), then track-object pose bookkeeping (identical to 0x68b330). |
| `PhysicsEngine::getAirDensity` | 0x140263a60 | 0x68ab90 (0x26a6de8) | C | Thread-local override (TLS +0x170, FLT_MAX = unset) else `1.2922 - 0.0041 * T(engine+0x100)`. A per-car value is therefore selectable per physics worker. |
| `PhysicsEngine::onCollisionCallBack` | 0x140264020 | 0x2db1f0 (0x26a6e08) | C | Empty function: acs's contact callback is disabled. |
| lookups | | | D | `getCore` 0x140263a80 (0x268b3d8), `isTyreLegal` 0x140263f30 (0x268b3e0), `initLowSpeedFF` 0x140263c90 (0x268b3e8), `stepPaused` 0x140265320 (0x268b3f0, called by CSP's paused branch), `stepWind` 0x140265380 (0x268b3f8). |
| `PhysicsDriveThread::PhysicsDriveThread` | 0x140122a60 | 0x6865b0 -> 0x6865e0 (0x268b3c8) | B | Original constructor via slot, then CSP thread-side init 0x68a380. |
| `PhysicsDriveThread::run` | 0x140123150 | 0x68a230 (0x26a6dd0) | C | One-time init 0x68a090, a scope object (0x40000000 = thread priority / MMCSS class: "New thread: AC: physics, priority: 2, class: Pro Audio" [log-csp]), then loops 0x689360 until the thread's shutdown flag (+0x38) or `DAT_18268b39b`. 0x689360 (246 B) polls DirectInput, `DICommandManager::step`, and in the paused case `stepPaused` + sleep. |
| `PhysicsDriveThread::step` #2 | 0x140123560 | 0x68a200 (0x26a6dc8, never called) | C | The real step loop, see 2b step 1-2. |
| `PhysicsDriveThread::step` #1 | 0x140123560 | 0x68a1d0 (0x26a6dc0, never called) | C | Variant for the timer-process mode: repeats the same 0x68abe0 / 0x68acd0 / 0x68b330 / handlers for about 10 ms of wall time (GetTickCount64 + 10) per call, core step deferred when `PARALLELIZED_CONTACTS`; scope name "AI test". Two registrations of the same symbol: see open question 1. |
| `PhysicsAvatar::onPhysicsStepCompleted` | 0x1401221a0 | 0x683080 (0x26a6d38) | A | Fires CSP event list `0x2691058` with the double 0.003 (dt) **before** the original state copy. |
| `PhysicsAvatar::pausePhysics` | 0x140122300 | 0x682d70 (0x268b3b0) | B | Passes `true` only when `DAT_182697c0f` is clear; otherwise forces `false`: physics is not paused while that flag is set (CSP session that must keep running, interp). |
| `PhysicsAvatar::render` | 0x140122320 | 0x682dc0 (0x26a6d28) | C | Replaced by CSP's per-frame update of its own physics-side render objects (vector `0x25e1628`, stride 0x18) when `this+0x3fc` is set. |
| `PhysicsAvatar::startPhysics` | 0x1401223a0 | 0x682e80 (0x26a6d30) | B | While the Embree build runs (`0x26a4cff`) it shows the loading item "Physics" / "Waiting for Embree" and sleeps in 5 ms slices; sizes a per-car vector (0x560 B/car); then the original (thread starts). |
| `PhysicsAvatar::stepCommandQueue` | 0x1401223e0 | 0x682d90 (0x26a6d20) | A/B | Pumps CSP's task scheduler (`0x1550bf0`, argument 0.003) then the original queue. |
| `PhysicsAvatar::update` | 0x140122480 | 0x683130 (0x26a6d40) | B | Skipped entirely (no state copy to the car models) while (online || `DAT_182697c0f`) and the client's +0x148 flag are set. |
| `PhysicsAvatar::shutdown` | 0x140122360 | 0x683160 (0x26a6d48, never called) | C | Sets global `0x2695016`, `DirectInput::cleanup` (lookup 0x268abe8) when the input object exists, sets the thread's shutdown flag (+0x378) and sleeps 50 ms; does not join. |
| `PhysicsAvatar::~PhysicsAvatar` | 0x140120d20 | 0x2db1f0 | C | Empty function (no destruction). |

### 2.3 Track objects, colliders, controllers, small step classes

| Symbol | acs VA | CSP RVA (slot) | Cat | What it changes |
|---|---|---|---|---|
| `PhysicsObject::PhysicsObject` | 0x1402ac8a0 | 0x68d500 (0x26a6e10) | B | 1,039 lines. Before the original it welds / reduces the object's collider triangles ("Track object collider `%s` optimized from %d to %d triangles"), gated by `OPTIMIZE_COLLIDERS` (global `0x268b411`); special-cases the three objects `AC_POBJECT_026/027/028` of one track (wide-string compare with `ks_barcelona` fragments) by substituting the mesh of a child node; then the original. |
| `PhysicsObject::setWorldMatrix` | 0x1402acb90 | 0x68f230 (0x26a6e18) | B | Original, then calls the rigid body's +0x10 with 0 and +0x70 with 1.0 (`stop`): a teleported track object also loses its velocity. |
| `CarColliderManager::loadINI` | 0x1402a37a0 | 0x426160 (0x2699850) | B | With `EXTEND_COLLIDER_BOXES` (global `0x2695 08c`) and a car being loaded: reads `car.ini [BASIC] INERTIA` and suspensions `BASEY`, sets the CSP global `0x268b900` (minimum box half-height, `max(0.2*y, 0.5*inertia.y)`) for the duration of the original call, then restores it. |
| `CarColliderManager::step` | 0x1402a3d50 | lookup (0x268a7d0) | D | Called by CSP's `stepComponents` (item 19). |
| `DynamicController::DynamicController` | 0x1402af330 | 0x5c0890 -> 0x5c0920 (0x268acc0) | A | The 5,505-byte body calls the original and registers the controller in a CSP hash map; it also contains the input-name table (`BRAKE`, `STEER`, `SPEED_KMH`, `SLIPRATIO_*`, `SLIPANGLE_*`, `OVERSTEER_FACTOR`, `REAR_SPEED_RATIO`, `STEER_DEG`, `WHEEL_STEER_DEG`, `LOAD_SPREAD_*`, `SUS_TRAVEL_*`, `AVG_TRAVEL_REAR`, ...) used to resolve extra input names [dll strings]. |
| `DynamicController::~DynamicController` | 0x1402b07c0 | 0x5c0760 (0x268acd0) | A | Removes the object from that hash map (`DAT_1825e1540`, lock byte `0x25e1568`), then the original. |
| `DynamicController::getInput` | 0x1402b0d70 | 0x5c1eb0 (0x268ace0) | B | If ext exists and the input id is known to ext's table (0x7fe490) it returns that value (script / CSP input), otherwise the original. |
| `DynamicController::eval` | 0x1402b0c00 | lookup (0x268acd8) | D | |
| `SpeedLimiter::init` | 0x1402bb900 | 0x6a2740 (0x26a7028) | B | Original, then clears 3 bytes of state (+2, +4). |
| `SpeedLimiter::step` | 0x1402bb910 | 0x6a2760 (0x268b5c8) | B/C | If `[EXTRA_RULES]` has the pit limiter speed (`_DAT_1826a9690+4`, default 80 km/h) different from 80 or the limiter flag +2 set, CSP runs its own rule: when speed > limit: sets `Engine::electronicOverride` (Car+0x5e8) = 0, and above limit+1 km/h writes an optimal-brake value (hook 0x10eb630) into BrakeSystem (+0x2ca8); with a tyre-damage check over the 4 tyres and a speeding-penalty counter; otherwise the original. |
| `Telemetry::step`, `AutoBlip::step`, `AutoShifter::step`, `DriftModeComponent::step`, `EDL::step`, `StabilityControl::step`, `SteeringSystem::step` | 0x1402bf5b0, 0x1402b9ef0, 0x1402ba7f0, 0x1402bff20, 0x1402bb460, 0x1402bfa50, 0x1402b81b0 | lookups (0x268a810, a7c0, a7c8, a7d8, a7e0, a800, a808) | D | Called by CSP's `stepComponents` through the slot, unchanged. They are therefore not replaced, but `Telemetry`/`Drift` run only for GUID 0 as in acs. |
| `TimeTransponder::step` | 0x1402911f0 | 0x6a5e70 (0x26a74c0, never called) | C | Re-implemented (120 lines). Per timeline of the track (stride 0x38, `TimeLine::check` through slot 0x268a7b8 [interp]) it detects crossings from the tyre world positions (Car+0x1120/0x1128), and accumulates time as `dt*1000 + fractional carry` (carry array `0x26a70c0[carIndex]`) so lap/sector times have 1 ms precision (`PRECISE_TRANSPONDER`, global `0x268b680`). |
| `TimeTransponder::onTimeLinePassed` | 0x140290c20 | 0x6a5b70 (0x26a70b0) | B | With `PRECISE_TRANSPONDER`: interpolates where between two steps the line was crossed (positions at t and t-0.003, using the 3-step odd-time offset `stepCounter % 3`) and adjusts the lap timer by up to +-3 ms before the original. |
| `TimeTransponder::lap` | 0x140290a50 | 0x6a5ab0 (0x26a70a0) | A/B | Original, then increments CSP's per-car lap counters (record+8: +0x2a4, and +0x294 decremented) unless the car is in the pit lane with counter 0. |
| `TimeTransponder::init` | 0x1402907c0 | 0x6a5b40 (0x26a70a8) | B | Original; clears +0x91; with `ARM_FIRST_LAP` (global `0x268b682`) sets +0x80 = 1 (armed on a hot-lap spawn in practice). |
| `TimeTransponder::reset` | 0x140290e60 | 0x6a61c0 (0x26a74c8) | B | For GUID 0 with fewer than 10 cars (`_DAT_1826add30 < 10`) or the keep flag (`0x268b681`): saves the 0x18-byte lap data around the original so a reset does not clear it. |
| `TimeTransponder::getCuts`, `isValid`, `split` | 0x1402906a0, 0x140290980, 0x140291150 | lookups (0x268b688, b690, b678) | D | |
| `PerformanceMeter::step` | 0x14026b8e0 | 0x682720 (0x26a6d00, never called) | C | Re-implemented sampler; the number of samples per lap adapts to the track length (`ADAPTIVE_PERF_METER`; base 1000 samples or half the AI-spline length, minimum 20), stores `{time, speed}` pairs, drives the delta number. GUID 0 only as in acs. |
| `PerformanceMeter::reset` | 0x14026b880 | 0x6826b0 (0x26a6cf8, never called) | C | Clears the current-lap vector; clears the reference lap too when (>= 10 cars and `DAT_18268b399` clear) or not GUID 0. |
| `PerformanceMeter::getCurrentSplit` | 0x14026b700 | lookup (0x268b390) | D | |
| `PenaltyManager::step` | 0x140265e10 | 0x682420 (0x268b388) | B | Clears the black-flag latch when the throttle-like Car value (record+0x984) is within 0.2 of 0.5; with `[EXTRA_RULES]` pit-speed rules (`_DAT_1826a9690+0x2c/+0x20/+0x30`) can set a penalty flag when the car is in the pit lane and farther than the rule distance; skips the original while that flag is on, until the car has left the pit lane. |
| `PenaltyManager::addJumpStartPenalty` | 0x140265a90 | lookup (0x268b380) | D | |

### 2.4 Components stepped inside `stepComponents` that belong to other briefs (class only, for the tick)

Hook RVA and category as seen in the same decompile survey (so the tick can name them; the owners of those classes should confirm): `BrakeSystem::step` 0x108eef0 (B, ext-aware), `Tyre::step` 0x6ac2a0 (B, 2,544 lines), `HeaveSpring::step` 0x5eb720 (B), `DRS::step` 0x426480 (B), `AntirollBar::step` 0x58be00 (B), `ABS::step` 0x563dc0 (B: with ext uses its own 4-channel slip model, else original), `TractionControl::step` 0x6a7cd0 (B) + ext wrapper 0x6a7d40 ("Modified TC slip" / "Initial TC slip"), `Drivetrain::step` 0x5bba80 (B: with a cphys drivetrain object it re-maps the clutch pedal Car+0x170 through a curve, calls the original, restores it), `FuelLapEvaluator::step` 0x5dbce0 (B), `LapInvalidator::step` 0x621840 (B), `SetupManager::step` 0x10f4f10 (B; ext variant 0x10f4f80), `Wing::step` 0x6bba00 (B), `SuspensionStrut::step` 0x6a3a50 and `SuspensionAxle::step` 0x6a3490 (hooks); `Suspension::step` is a lookup.

### 2.5 `Sim::` hooks that touch the physics clock

| Symbol | acs VA | CSP RVA (slot) | Cat | What |
|---|---|---|---|---|
| `Sim::update` | 0x14019ef90 | 0x11bd380 (0x26add58, never called) | C | Pumps CSP's main-thread scheduler (`_DAT_1826add08`, 0x1550bf0), runs a CSP copy of the acs body (0x11bcf00 contains the "CPU OCCUPANCY > 99%" warning, `ACCameraManager::setMode` and the `ReplayManager::updatePlayState` call), then fires CSP event list `0x26978e8` with dt. |
| `Sim::executeOnMainThread` | 0x140199240 | 0x11bce70 (0x26add50) | C | Re-routes the closure into CSP's scheduler instead of acs's `chFunctions` queue. |
| `Sim::setPauseMode` | 0x14019e6d0 | 0x11be250 (0x26add78) | B | Event list `0x26978f0` with the new state; any handler can veto (returns before the original). |
| `Sim::onReplayModeChanged` | 0x14019e0c0 | 0x11bfa00 (0x26addb8) | B | The acs "sleep 500 ms on entering replay" becomes 20 ms when global `0x25a2e21` is set; entering replay pauses physics only when no client **and** `DAT_182697c0f` clear; leaving sets a flag byte (+0x74) of the replay manager. Event list `0x26978a8` can change the delay. |
| `Sim::startGame` | 0x14019eb00 | 0x11be370 (0x26add80) | B | Event list `0x26978d0` can veto the camera fade-in when `race.ini` game state == 1. |
| `Sim::shutdown` | 0x14019e9e0 | 0x11bf830 (0x26addb0) | B | `DAT_18268b39b` = 1 (shutdown flag read by `pollControls` hook #2 and the physics thread loop), flushes scheduler with 0.003, shuts workers (0x6a0170), sets it to 2, then the original. |
| `Sim::Sim`, `onKeyDown`, `activateGameGui`, `activatePauseMenu`, `setSplashMessage`, `getNationFlag`, `applyCustomWeather` | | | other briefs | not about the clock. |

## 2a. The per-car objects and global flags the hooks test

**Where the "extended" state lives (what the owner asked for).**

| What | Where | Evidence |
|---|---|---|
| `physicsGUID` | Car+0x3238 (u32) | [dll] gates the GUID-0 list in `stepComponents` exactly like docs/map/car_step.md 3.4; `Car::step`, `TimeTransponder`, `PerformanceMeter` test it |
| second car index | Car+0x3e9c (u32) | [dll] 46 hooks index CSP's vectors with it; `getSteerFF`/`TimeTransponder::step` test it against 0; for the player car both are 0. Not in docs/map. [interp] CSP stores its own index there (acs reserved bytes at the Car tail) |
| CSP per-car record (`ac_car_info`) table | vector at dwrite.dll RVA 0x26ae278..0x26ae280 | [dll] indexed by GUID (Car::step, reset, forcePosition) or by +0x3e9c; entry has name wstring at +0xe8 (used for profiler scope names), car index at +0xb0, Car* at +0x318, a sub-record at +0x8 with lap counters (+0x294, +0x2a4), per-car smoothing object at +0x88, a float at +0x478 (FF scale), shared-state at +0x548 |
| **extended-physics state (`ext`)** | vector at 0x26ae290..0x26ae298, entry = pointer, index = Car+0x3e9c | [dll] every one of the ~46 hooks does "look ext up; if non-null do the CSP path, else call the original". During loading `_DAT_1826a9d50` (the `ac_car_info` of the car under construction, `DAT_1825e2728` = "loading") gives `ext = *(info + 0xe0)` instead |
| fields of `ext` that were seen | | +0x000 dirty byte (queues a job to the physics thread), +0x0b0 car index, +0x1b8 / +0x1c0 / +0x1d8 / +0x1f0 / +0x210 / +0x220 / +0x228 / +0x240 / +0x250 / +0x258 / +0x268 / +0x270+8*i: sub-objects (steering model, ABS channels, pits/repair joints, air-density data, script holders, FFB tweaks, per-wheel `cphys` suspension objects), +0x310 "CPHYS Controller" script bridge, +0x318 Car*, +0x320 car record whose +0xf80 is the `RaceEngineer`, +0x418 body position, +0x434 ground wind, +0x524..0x54c step accumulators (zeroed every `stepComponents`), +0x59c/+0x604 two 0x6c snapshots (parity +0x674), +0x6d1 repair flag bits, +0x6d8 counter. All [interp] names. |
| **flags dword** | Car+0x3db4 | [dll] bits used: 0x2 (collider mask), 0x4 (extra smoothing), 0x20 (int16 steer at +0x3e8e), 0x40 (steer delay buffer), 0x80 (script annotates state), 0x100 (re-entrancy guard in forcePosition), 0x200 (collider clamp failed), 0x8000 (FF scale), 0x10000 (stationary auto-brake / sleeping at speed < 0.1), 0x20000 / 0x200000 / 0x400000 (state export tweaks), 0x80000, 0x1000000 (damage locked). Writers are in non-decompiled CSP functions (request). |

The three ways the object appears (answering "how does CSP get a per-car extended state"):

1. **Parsing.** `car.ini [HEADER] VERSION` is compared against the prefixes `extended-` and `extendedV2-` in two CSP functions, 0x918c80 (1,498 B, callers 0x91f970, 0x920650, 0x920f60, 0x921860) and 0x32c650 (9,705 B, the car-asset loader of "[CarSwap]", caller 0x337280) [dll strings]. The loader also names the files it opens: `colliders.ini`, `drivetrain.ini`, `engine.ini`, `suspensions.ini`, `tyres.ini`, `sounds.ini`, `lods.ini`, `wing_animations.ini` ... Neither is a hook (no table row) and neither is decompiled: the `N` of `extended-N` and where it is stored are **not read** (request).
2. **Attaching.** `Car::Car` hook 0x595b40 -> 0x595b70 -> 0x5a07e0 / 0x5a07c0 creates/attaches the CSP per-car object after the acs constructor (also called from 0x339f80, the car-swap path) [dll call graph].
3. **Presence for stock cars.** The CSP log of this PC prints "Cphys Drivetrain Initialized." and "CPHYS ERS not enabled; initialization skipped." for the F2004 [log-csp, prior]. So the cphys sub-objects are created for every car; "extended-N" decides which of them are real (COSMIC suspensions, script, K&C tables `/cphys/K&C/`, extra contact points, ...). Whether `ext` itself is non-null for stock cars is not proven; for them most hooks fall through to the original because the sub-object they test (`ext+0x250` script, `ext+0x1d8` ABS data, `lVar3 = 0x5bf9d0(drivetrain)`) is null. [interp]

**Global flags.** [dll] address RVAs in the dll data; names by behaviour [interp] unless marked.

| Global | Meaning |
|---|---|
| `DAT_18268b162` | profiling scopes on (per-section timers); also read by audio hooks |
| `DAT_18268b39b` | 0 normal, 1/2 shutting down (written by `Sim::shutdown`, [dll]) |
| `DAT_18268b39f` | online session (written by the `RaceManager::initOnline` hook 0x10edae0, [dll]) |
| `DAT_182697c0f` | a second "online-like" flag: pause suppression, controls lock, `getPhysicsState` zeroing (writer not decompiled) |
| `_DAT_1826adb40` (+0x148) | the network client / "waiting to start" flag used with the two above |
| `_DAT_1826a6de0` | the `PhysicsEngine*` (written in the `PhysicsEngine` constructor hook, [dll]); +0x20 = physicsTime, +0x100 = ambient temperature, +0x1a8 = step counter, +0x190 = core |
| `_DAT_1826a9690` | `[EXTRA_RULES]` settings, filled by function 0x945a70 which reads `EXTRA_RULES`, `CHANGE_TYRES_PER_AXIS`, `SPEEDING_SUBSEQUENT_PENALTY`, `SPEEDING_PENALTY`, `SLIPSTREAM_MULT`, `DISABLE_AUTOBRAKING`, `SLIPSTREAM_SPEED_FACTOR`, `SLIPSTREAM_GAIN_MULT` [dll]; other keys of the same block seen as strings: `PITS_SPEED_LIMITER`, `LIMIT_LOCK_CONTROLS_TIME`, `LIMIT_LOCK_CONTROLS_TOTAL_TIME`, `PITS_ORDER`, `RETAIN_BRAKES_TEMPERATURE_IN_PITS`, `BACK_TO_PITS_PRESERVES_DAMAGE`, `EMERGENCY_RESET`, `KEEP_COLLISIONS`, `REQUIRE_NEW_LAP_FOR_DRIVETHROUGH_PENALTY`, `UNFIT_FOR_DRIVETHROUGH_PENALTY` |
| bytes at `0x2695 087..0a5` | the `general.ini [PHYSICS_EXPERIMENTS*]` options in file order: ENABLE_MESH_COLLIDER_SOONER (`0x269508a`), CLAMP_MESH_COLLIDERS (`0x269508b`), EXTEND_COLLIDER_BOXES (`0x269508c`), PARALLELIZED_CONTACTS (`0x2695090`), OPTIMIZE_COLLIDERS (`0x268b411`), PRECISE_TRANSPONDER (`0x268b680`), ARM_FIRST_LAP (`0x268b682`), EQUALIZE_PITS_CHECK (`0x268bcb2`), SMOOTH_CONTROLS_LOCKING (`0x2695098`) — matched by order and by behaviour |
| `_DAT_1826add30` | [interp] number of cars; 10 or more switch several "keep player data" rules |

## 2b. The physics tick with CSP injection points

### 2b.1 One wake-up of the physics thread (outer loop)

`>>` marks a CSP injection (something acs.exe does not do), `==` marks code that is acs.exe's own and unchanged.

1. `>>` **Thread body** `PhysicsDriveThread::run` hook 0x68a230: one-time init, high-priority scope, then repeats 0x689360 until shutdown. `==` acs's own `run` is never entered.
2. `>>` **Poll and pause** (0x689360): `DirectInput::poll` (hook 0x5ae5c0), `DICommandManager::step` (hook 0x5a7260); if paused: `PhysicsEngine::stepPaused` (acs lookup) and a sleep; else the step function below.
3. `>>` **Clock** (0x68a200). `now = (QueryPerformanceCounter - base) * freq * timeScale` (+0x28). `currentTime` (+0x40) advances by 3.000000026 ms per step. If `currentTime <= now` there are steps to run:
   - for the first 100 engine steps (`engine+0x1a8 < 100`) a backlog is not replayed: `currentTime = max(currentTime, now - 1.5 ms)`;
   - afterwards only a backlog > 1000 ms (and `timeScale < 2`) is cut, with the log line "RESET PHYSICS TIMER" (same as acs; text built from constants);
   - if `currentTime > now` the thread waits (0x68a440: sleep / yield / high-resolution timer according to `[THREAD_TWEAKS] PHYSICS_TIMING`, "Overslept" statistics).
4. More than 5 steps in one wake-up or a step costing more than 3 ms are logged by 0x689b00 ("Late frame #%d ... K P C O F S Y") and counted; `iRam1826c8b48 < 100` limits the log.

### 2b.2 One 3 ms step (what 0x68a200 does per step; acs's `PhysicsEngine::step` order in brackets)

| # | CSP action | acs.exe equivalent | Injection |
|---|---|---|---|
| 1 | 0x68abe0 `(engine, 0.003f, currentTime, now)`: gameTime, physicsTime, stepCounter++ | head of `PhysicsEngine::step` 0x140264760 | `>>` scope "Preparing to compute" |
| 2 | evOnPreStep -> `PhysicsAvatar::stepCommandQueue` hook 0x682d90: **CSP scheduler pump (dt 0.003)** then acs's queued main->physics jobs | `PhysicsAvatar::stepCommandQueue` 0x1401223e0 | `>>` |
| 3 | 0x68acd0 "Computing cars" [not decompiled]: contains "Prepare spaces", the LLTP worker dispatch (0x594fc0 "LLTP (%d)" threads, 0x15f27c0 / 0x15fa7b0 task submit) and the call to the `Car::step` hook per car; Track::step / stepWind / `Car::stepPreCacheValues` (acs lookups) are called from here [interp] | Track::step 0x140278d20, stepWind 0x140265380, stepPreCacheValues 0x1402768c0, per-car `Car::step`, thread-pool tasks | `>>` own pool, own order of cars [interp] |
| 3a | **Per car, `Car::step` hook 0x5996f0** | `Car::step` 0x140275da0 | see 2b.3 |
| 4 | `core->step(0.003)`: CSP's ode64 core: collisions + joint solve + integrate. With `PARALLELIZED_CONTACTS` (byte `0x2695090`) the contact search was started concurrently with 3a (log text: "Collisions are being checked in parallel with cars") and the core step is finished here | `PhysicsCore::step` 0x1402cd690 / `dWorldStep` 0x1403404c0 | `>>` whole core replaced; `Car::onCollisionCallBack` must be called by CSP's own contact code [interp] |
| 5 | 0x68b330 "Synchronizing track objects": for every movable track object keeps a 4-entry pose history (ring of 4 x 0x40 bytes), flag +0x161 = awake, and sets the body's collision mask to 0x1f when awake, 0x0c when asleep | `PhysicsObject` lambda 0x1402acb00 (pose bookkeeping) | `>>` |
| 6 | evOnStepCompleted handlers (`Car::postStep` per car, `NetCarStateProvider::step`, `ACClient::onPhysicsStep`) | `PhysicsEngine::step` tail | [interp] run inside 0x68acd0 / 0x68b330; the loop is present verbatim in 0x68b580 but not in 0x68a200 (open question 2) |
| 7 | thread-level `evPhysicsStepCompleted` handlers (+0x10..0x18 of the thread): `PhysicsAvatar::onPhysicsStepCompleted` hook 0x683080 fires **CSP event `0x2691058` (dt 0.003)** and then the original: state snapshot under the lock (`PhysicsEngine::getPhysicsStates` -> `Car::getPhysicsState` hooks 0x59dfa0 / 0x59e920, `Car::getWingState` hook 0x59fff0), replay frame, `SharedMemoryWriter` (acs's writer methods are empty stubs, CSP supplies its own) | `PhysicsAvatar::onPhysicsStepCompleted` 0x1401221a0 | `>>` |
| 8 | Timing figures: preparing / cars / contacts / track-object sync / submit, each in ms, smoothed by 0.02 per step (CSP Debug page "Physics timing"); thread occupancy = sum*33.33 | `PhysicsDriveThread` occupancy | `>>` |

### 2b.3 Inside one `Car::step` (acs order, CSP injections marked)

| Step | What happens | CSP |
|---|---|---|
| 0 | profiling scope "Car n" (name from the per-car record) | `>>` |
| 1 | GUID 0: function 0x599000 (1,772 B: queues a closure to the physics thread through 0x59f870, allocates a scope, reads the settings object 0xef1700; [interp] player-car-only pre-step bookkeeping). GUID != 0: `framesToSleep = 250` (acs default 50) | `>>` request |
| 2 | ext lookup (`_DAT_1826ae290[Car+0x3e9c]`, or `*(ac_car_info+0xe0)` while loading). If null: **original `Car::step` unchanged**, scope closed | `>>` |
| 3 | ext present: body position -> ext+0x418; toggle parity and copy the 0x6c-byte snapshot into ext+0x59c/0x604; if ext's car index is 0: per wheel i compute steering geometry from the CSP suspension objects (ext+0x270+8i) and write `wheelSteerAngle` values into the global value block (0x7381c0) and the wheel objects (ext+0x1f8 +0xd8/+0xe0), unless `ext+0x308` object disables it; if `ext[0]` is set queue a re-sync job | `>>` |
| 4 | **original `Car::step`**, inside which the acs order is (docs/map/car_step.md 3.3): ERP/CFM switch (GUID 0) -> `Car::pollControls` **hook #2** (original poll, then flags 0x10000/0x20/0x40, then script update 2c) -> lights action -> black-flag teleport (`Car::isInPits` hook; `Car::forceRotation` lookup; `Car::forcePosition` hook) -> `Car::updateAirPressure` **hook C** -> fuel burn -> `updateBodyMass` (once per second) -> control overrides / penalties / lock (`lockControlsUntil` hook) -> steer signal -> `Autoclutch::step` -> sleeping logic (framesToSleep 250 for cars != 0) -> accG -> `stepThermalObjects` -> **`Car::stepComponents` hook C (2b.4)** -> `Car::updateColliderStatus` hook B -> GUID 0: `stepJumpStart` (`PenaltyManager::addJumpStartPenalty` lookup) | `>>` hooks |
| 5 | after the original: if `ext+0x240` is set, `ext->0x318->engine.velocity > 0` and `->0x138->0x108 > 0`: three calls of 0x82d510 (three 0x10-byte objects) [interp: extra drag/torque from an "engine component"]; then 0x597f00 (4,117 B: the biggest CSP-side post-step; calls the value block 0x7381c0 seven times, allocs, 0x597400 which holds the strings `cphys_data` and `ac_ier_mem`); then 0x10eb8f0 with ext+0x424 | `>>` request |
| 6 | scope closed (adds elapsed time / count, clears the owning thread id) | `>>` |

### 2b.4 `Car::stepComponents` hook 0x599ec0: all 29 components (order identical to docs/map/car_step.md 3.4)

Items marked `>>` add CSP work; "orig" means the acs function is called through its slot or lookup.

| # | Component | acs VA | Called as | CSP injection |
|---|---|---|---|---|
| 0 | prologue | | | Car+0x82c := Car+0x5e8 (engine electronic override copy); scope "Car #1/Components" (GUID 0 only) |
| 0b | ext pre-work | | | `>>` control smoothing while locked (`SMOOTH_CONTROLS_LOCKING`: steer eased 5 %/step to the stored value, brake 1.0 online), steerAngle (Car+8), `ext+0x310` CPHYS-controller script bridge `0x41f240` then the global value block (+0x2494/+0x2498), `ext+0x230` component step 0x870b00, wheel axle objects 0x8d8270 on tyres 0 and 2 |
| 1 | BrakeSystem::step | 0x14028e640 | CSP hook 0x108eef0 | B (ext-aware, `ac.overrideBrakesTorque`) |
| 2 | EDL::step | 0x1402bb460 | orig (slot 0x268a7e0) | none |
| 2b | | | | `>>` ext accumulators ext+0x524..0x54c zeroed |
| 3 | suspension->step x N (ISuspension +0xb0) | 0x1402c3390 / 0x1402c6600 / 0x1402c8770 / 0x1402cab00 | vtable call; Strut/Axle have CSP hooks 0x6a3a50 / 0x6a3490; COSMIC (type 5) is a CSP class | scope "Car #1/Suspensions" |
| 4 | Tyre::step x4 | 0x140283800 | CSP hook 0x6ac2a0 | scope "Car #1/Tyres"; **the acs callback that fires `onTyresStepCompleted` after tyre 3 is not used**: CSP calls it itself in 4b |
| 4b | steering / FF | | | `>>` ext[0x38] steering model (0x876520 / 0x876fe0: rack-force integrator, uses `|Car+0x3e34|` curve lookups, two modes: plain and "physical", the latter drives joints of the steering body), then **`Car::onTyresStepCompleted` through the lookup (slot 0x268aae8)**, then a per-wheel loop that tracks the running maximum of each suspension's status float and, when it rises, calls 0x601260 with `(delta * engine+0x108 * ext+0x13c)` [interp: suspension impact damage] |
| 5 | HeaveSpring::step x2 (k != 0) | 0x1402b3960 | CSP hook 0x5eb720 | |
| 6 | DRS::step | 0x1402b4e60 | CSP hook 0x426480 | |
| 7 | AeroMap::step | 0x1402b7150 | CSP wrapper 0x5850c0 -> orig slot 0x268a8b8 (`Wing::step` is hook 0x6bba00) | scope "Car #1/Aero" |
| 8 | Kers::step (present) | 0x1402b7e10 | orig (slot 0x268b220) | `>>` CSP event list `0x2690cb0` (each 0x28-byte handler called with the Kers object) before it |
| 9 | ERS::step (present) | 0x1402930e0 | thunk 0x5ccdd0 -> orig slot 0x268ad70 | |
| 9b | ext repair bits | | | `>>` `ext+0x6d1` bits 1, 2, 4, 8 consumed; bits 4/8 call suspension index 2/3 method +0x28 (re-attach after repair) |
| 10 | SteeringSystem::step | 0x1402b81b0 | orig (slot 0x268a808) | |
| 11 | AutoBlip / AutoShifter / GearChanger step | 0x1402b9ef0 / 0x1402ba7f0 / 0x1402bab50 | orig (slots 0x268a7c0 / a7c8 / a7e8 [slot a7e8 has no table row, interp]) | |
| 14 | Drivetrain::step | 0x14026b130 | no cphys data: orig slot 0x268aca0; else CSP 0x5bbab0 | `>>` |
| 15 | AntirollBar::step x2 | 0x1402bb640 | CSP hook 0x58be00 | `>>` for suspension type 5 the loop copies the ARB link object between the two hubs (RTTI cast to `cphys_suspension_cosmic`) |
| 16 | ABS::step | 0x14028f610 | CSP hook 0x563dc0 | |
| 17 | TractionControl::step | 0x140290200 | ext: CSP 0x6a7d40 ("Car #1/Traction control"); else orig slot 0x268b6b8 | |
| 18 | SpeedLimiter::step | 0x1402bb910 | CSP hook 0x6a2760 (calls 0x1806a2760 directly) | |
| 19 | CarColliderManager::step | 0x1402a3d50 | orig (slot 0x268a7d0) | |
| 20 | SetupManager::step | 0x14028d090 | ext: CSP 0x10f4f80; else orig slot 0x268c288 | |
| 21-25 | GUID 0 only: Telemetry::step, DriftModeComponent::step, PerformanceMeter::step, LapInvalidator::step, PenaltyManager::step | 0x1402bf5b0, 0x1402bff20, 0x14026b8e0, 0x1402c0580, 0x140265e10 | orig; orig; **CSP 0x682720 (C)**; hook 0x621840 (B); hook 0x682420 (B) | after LapInvalidator: a CSP block (global byte `0x26950fe`) sets `lapInvalidator.car->+0x27c = 1` once when `Car::isInPitLane` and clears a CSP flag at `_DAT_182690f58+0xb4` [interp: ARM_FIRST_LAP / pit-lane bookkeeping] |
| 26 | SplineLocator::step | 0x1402ab5c0 | CSP 0x6a2ca0 -> orig lookup (slot 0x268b5f0) [interp] | |
| 27 | StabilityControl::step | 0x1402bfa50 | orig (slot 0x268a800) | |
| 28 | TimeTransponder::step | 0x1402911f0 | **CSP hook 0x6a5e70 (C)** | 1 ms transponder |
| 29 | FuelLapEvaluator::step | 0x14028d360 | CSP hook 0x5dbce0 | accumulates (distance, time) when 50 < v < 1000 km/h |

### 2b.5 What changes at which physics step (summary)

| When | What |
|---|---|
| Every 3 ms step, every car | the `Car::step` wrapper; `stepComponents` replacement; `updateAirPressure` replacement; `updateColliderStatus` post-work; `pollControls` post-work; `TimeTransponder::step`; GUID 0: `PerformanceMeter::step` |
| Every step, ext cars | body position record, snapshot parity copy, ext accumulators reset, script `update(dt)` (1 call per script holder), controller bridge, steering model, suspension-damage tracker |
| Every step, other cars (GUID != 0) | `framesToSleep = 250` (stationary AI / remote cars take 250 steps = 0.75 s to be frozen) |
| First 100 engine steps | no backlog catch-up (clock clamped to 1.5 ms) |
| Once a second | acs's own `updateBodyMass` (unchanged) |
| Each lap / line crossing | `onTimeLinePassed` fractional correction (+-3 ms); `lap` per-car counters |
| On `forcePosition` / `reset` / repair | CSP joint release, script holder reset, event lists `0x268ff50`, `0x268ff58`, repair bits for `stepComponents` |
| Every `getPhysicsState` (once per step after the step) | tyre-block overwrite, accG low-pass 0.25, flag-driven state tweaks, optional replay/override provider |
| Online / server rules | `lockControlsUntil` caps, pit limiter speed, slipstream multipliers, `PITS_ORDER` pit time |

## 2c. How the Lua physics script is invoked each step

[dll] chain: `Car::pollControls` hook 0x59fe60 (`slot 0x26a47f8` called first) -> for the two script holders `ext+0x258` and `ext+0x250` (when present) function **0x856ed0** (2,197 B; argument = holder+0x68, dt) -> if not loaded: **0x1373420** (2,819 B; strings "Initializing", "Loading Lua script [", "Reloading Lua script [", "Delaying loading of Lua script") ; readiness test 0x1375390; then **0x1375740** (1,297 B; strings "Lua/%s" = profiler name, "__withAsyncUpdate") whose callees 0x136d110 (2,263 B) and 0x1375c60 (1,445 B) are the actual Lua call and are not decompiled. The Lua instance is a `lua::limp_physics_thread` (RTTI), the script type is `cphys_script::cphys::impl::cphys_script_limp`; the library is the 135-library overlay archive's `ac_car_cphys`, mapped by `rules.json` to `content/cars/*/data*` (no file I/O: `withoutIO`) [sdk]. Per step the order is: acs poll -> script update -> the rest of `Car::step`, so script outputs (`ac.overrideGasInput`, `ac.addForce`, `ac.overrideBrakesTorque`, `ac.overrideEngineTorque`, ...) apply in the same step. Other effects of the script are consumed later: FF value in `getSteerFF`, brake torque in the `BrakeSystem::step` hook, ABS/TC overrides in their hooks, extra masses in `stepComponents`/`updateBodyMass`. `Car::pollControls` hook #1 (0x59fdb0) is an alternative "script drives the car" path that calls 0x1373420 and 0x1375740 directly for `ext+0x250`.

## 3. Config keys and Lua API

**`extension/config/general.ini`** (keys that map to the hooks above; shipped defaults and one-line comments in my words) [ini]:

| Key (section) | Default | Hook it matches | Meaning |
|---|---|---|---|
| `EXTENDED_PHYSICS` (`[PHYSICS_EXPERIMENTS]`) | 0 | the ext presence (2a) | use extended physics for all cars and tracks |
| `ENABLE_MESH_COLLIDER_SOONER` | 1 | `updateColliderStatus` | mesh collider active when the car is not driving straight, may reduce performance, changeable live |
| `CLAMP_MESH_COLLIDERS` | 1 | `initColliderMesh` | mesh may not go below the box colliders |
| `EXTEND_COLLIDER_BOXES` | 1 | `CarColliderManager::loadINI` | stops cars falling through the ground at speed |
| `DISABLE_AUTOBRAKING` | 0 | `pollControls` / `isSleeping` | 0 original freeze, 1 realistic (handbrake needed), 2 adapted, 3 auto-braking |
| `AWAKE_IN_PITS` | 2 | `getPhysicsState`/`isSleeping`/ext joints | 0 off, 1 awake, 2 fully dynamic (tyre grip off while holding the car) |
| `STALLING`, `REPAIR_WHEELS2`, `EXTRA_CONTACT_POINTS`, `PHYSICS_SHADOWS` | 0, 0, 1, 0 | ext paths | see prior `csp_survey.md` P23/P24 |
| `PRECISE_TRANSPONDER` | 1 | `TimeTransponder::step/onTimeLinePassed` | 1 ms lap timing |
| `ARM_FIRST_LAP` | 1 (hidden) | `TimeTransponder::init` | arm the first lap on hot-lap spawns in practice |
| `ADAPTIVE_PERF_METER` (`[OTHER_EXPERIMENTS]`) | 1 | `PerformanceMeter::step` | more samples on long tracks |
| `OPTIMIZE_COLLIDERS`, `TWEAK_TRACK_OBJECTS`, `EQUALIZE_PITS_CHECK` | 1, 1, 1 | `PhysicsObject::PhysicsObject`, `Car::isInPits` | track-object collider optimisation, pit check thresholds |
| `PARALLELIZED_CONTACTS`, `MT_ODE_STEP`, `HASH_SPACE_TRACK`, `HASH_SPACE_CARS`, `ODE_BATCH_RAYCAST` | 1, 1, 1, 1, 0 | tick step 3/4 | contact search while cars compute, parallel joints solver, hash spaces, Embree vs ODE tyre rays |
| `SMOOTH_CONTROLS_LOCKING`, `CUSTOM_POBJECTS_MOTION`, `ALTERNATIVE_OPTIMAL_BRAKE` (`[PHYSICS_EXPERIMENTS_DONE]`) | 1 | `stepComponents`, `PhysicsObject`, `SpeedLimiter` | always on |
| `PHYSICS_TIMING`, `MMCSS_PRIORITY_PHYSICS`, `PROCESS_PRIORITY` (`[THREAD_TWEAKS]`) | 4, 1, 1 | thread loop | wait strategy and priority of the physics thread |
| `[STEERING_EXTRAPOLATION] PHYSICS_LEAD_TIME` | 0 | `getSteerFF` / controls | lag compensation (0 = off) |

**Server `[EXTRA_RULES]`** keys seen in the dll: `PITS_SPEED_LIMITER`, `CHANGE_TYRES_PER_AXIS`, `SPEEDING_PENALTY`, `SPEEDING_SUBSEQUENT_PENALTY`, `LIMIT_LOCK_CONTROLS_TIME`, `LIMIT_LOCK_CONTROLS_TOTAL_TIME`, `REQUIRE_NEW_LAP_FOR_DRIVETHROUGH_PENALTY`, `UNFIT_FOR_DRIVETHROUGH_PENALTY`, `BACK_TO_PITS_PRESERVES_DAMAGE`, `EMERGENCY_RESET`, `KEEP_COLLISIONS`, `RETAIN_BRAKES_TEMPERATURE_IN_PITS`, `PITS_ORDER`, `DISABLE_AUTOBRAKING`, `SLIPSTREAM_MULT`, `SLIPSTREAM_GAIN_MULT`, `SLIPSTREAM_SPEED_FACTOR`, `RIGID_DIRT_COLLISIONS`, `RIGID_WALLS_COLLISIONS`, `RIGID_FLOOR_COLLISIONS` (the last three are track `surfaces.ini` keys) [dll strings, 0x2247cf8..0x2248158].

**Track `surfaces.ini`**: `[_SCRIPTING_PHYSICS]` (`ALLOW_TRACK_SCRIPTS`, `ALLOW_DISPLAY_SCRIPTS`, `ALLOW_NEW_MODE_SCRIPTS`, `ALLOW_TOOLS`) and `[SURFACE_0] WAV_PITCH=extended-0` [sdk].

**Car data** (inside `data.acd`, not opened): `car.ini [HEADER] VERSION=extended-N | extendedV2-N`, `suspensions.ini TYPE=COSMIC`, `colliders.ini` (`COLLIDER_n CENTRE/SIZE`, CSP keys `MESH_OFFSET`, `*_EXTENSION`), `drivetrain.ini`, optional `script.lua` (+ `script_*.lua` modules), `/cphys/K&C/` kinematics tables [dll strings].

**Lua API (physics script)**: SDK library `ac_car_cphys`: 992 functions (197 own, the rest shared with the other libraries), 64 classes (`ac.StateCphysCar`, `ac.StateCphysWheel`, `ac.StateCphysDamper`, `ac.StateCphysSurface`, `ac.StateCarPhysics`, `ac.StateWheelPhysics`, `ac.StateWingPhysics`, `ac.CarExtraSwitchParams`, ...) [sdk `lua_sdk_counts.tsv`]. Physics-write entry points present in `lib.lua` (47 names grepped: `ac.accessCarPhysics`, `ac.accessCarDamper`, `ac.addForce`, `ac.addTorque`, `ac.addHubForce`, `ac.addCosmicForce`, `ac.addCosmicTorque`, `ac.addElectricTorque`, `ac.addWheelsFeedbackTorque`, `ac.overrideABS`, `ac.overrideBrakesTorque`, `ac.overrideEngineTorque`, `ac.overrideGasInput`, `ac.overrideTurboBoost`, `ac.overrideTurboBlowOffValve`, `ac.overrideTyreIgnitionPoint`, `ac.overrideTyreSmoke`, `ac.overrideCarState`, `ac.overrideSpecificValue`, `ac.setExtraMass`, `ac.setExtraTorque`, `ac.setExtraAIGrip`, `ac.setEngineRPM`, `ac.setEngineRPMIdle`, `ac.setEngineRPMLimit`, `ac.setEngineLifeLeft`, `ac.setEngineStalling`, `ac.setEngineStarterTorque`, `ac.setEngineBrakeSetting`, `ac.setClutchMaxTorque`, `ac.setTurbo*`, `ac.setDrivetrainDamageRPMWindow`, `ac.disableEngineLimiter`, `ac.disallowExtraSwitch`, `ac.setExtraSwitch`). Script callbacks named in the SDK: `script.update(dt)`, `script.reset()`, `ac.pauseScriptUntil()` / `ac.ScriptResumeCondition` (script can skip steps until an event) [sdk]. Not listed here: the general API shared with the other libraries (another brief).

## 4. Files it reads

- `content/cars/<car>/data.acd` or `data/`: `car.ini` (`[HEADER] VERSION`, `[BASIC] INERTIA`), `suspensions.ini`, `colliders.ini`, `drivetrain.ini`, `engine.ini`, `tyres.ini`, `script.lua` and `script_*.lua` [dll strings; cars in this install: 3 of 204 have physics scripts and COSMIC suspensions, `vrc_formula_alpha_2025_csp`, `vrc_formula_alpha_2026_csp`, `vrc_formula_beta_2024_csp`, with 36 / 2 / 31 script files, from `content_scan.json`; data.acd is the standard AC container and was not opened].
- `extension/config/general.ini` and `Documents/Assetto Corsa/cfg/extension/general.ini`.
- Server welcome / extended config (online `[EXTRA_RULES]`).
- A stored car-state file `<car unix name>preview_state*` for the preview feature (`getPhysicsState` #1).
- Track `surfaces.ini` (`WAV_PITCH=extended-N`, `[_SCRIPTING_PHYSICS]`, `COLLIDER_*`, `RIGID_*` keys) and `content/tracks/<track>/extension/ext_config.ini` for track objects.
- `system/cfg/assetto_corsa.ini` as in acs (the CSP log shows CSP overrides the thread count: "CSP uses 7 workers").

## 5. How it is switched on

- Always on (no per-car gate): the physics core replacement, the thread loop, `PhysicsEngine::step`, `Car::stepComponents`, `updateAirPressure`, `getWingState`, `isSleeping`, `TimeTransponder::step`, `PerformanceMeter::step`, the empty `onCollisionCallBack` [dll].
- Per car: the extended state `ext` (2a) and, within it, per feature sub-objects; script, COSMIC suspensions, K&C tables from `car.ini VERSION=extended-N` / `extendedV2-N`. Three installed cars qualify; the F2004 used in the earlier survey has `VERSION=1`, so it runs with the stock sub-objects only [prior].
- Per track: `surfaces.ini [SURFACE_0] WAV_PITCH=extended-N`.
- Global: `general.ini [PHYSICS_EXPERIMENTS] EXTENDED_PHYSICS=1`, or a new mode `FORCE_EXTENDED_PHYSICS=1` [ini, prior].
- Online: `[EXTRA_RULES]` from the server.
- Defaults on this PC [prior, ini]: `EXTENDED_PHYSICS=0`; the experiment keys above at their shipped defaults; the owner changed no physics key.

## 6. Size

- 58 hooks + 26 lookups in the classes of this file; 231,056 bytes of decompiled pseudo-C; `Car` 26 hooks + 7 lookups, 138,023 bytes, largest `Car::stepComponents` (24,286 B), `Car::getPhysicsState` #2 (16,413 B), #1 (16,040 B), `Car::pollControls` #1 (15,695 B), `Car::initColliderMesh` (15,841 B), `PhysicsObject::PhysicsObject` (39,244 B).
- Not decompiled but called from them (the unknown half): the 40-odd functions named in "Decompile requests", about 60 KB of code (sizes from `dll_funcs.tsv`; the three biggest are 0x410de0 11,410 B, 0x32c650 9,705 B, 0x84f430 7,200 B).
- RTTI of CSP's physics core: 20 `cphys::` types (`ode64_core`, `ode64_rigidbody`, `ode64_dynamic_collider`, `ode_impl_core`, `ode_impl_raycaster`, `ode_impl_joint_base`, `ode_impl_damper/pits/repairing/shifting_joint_base`, `ode_impl_linear/angular_motor_base`, `non_joint`, `cphys_suspension_cosmic`, `cphys_suspension_dummy`, `cphys_script_limp`, `cphys_values_base`, `virtual_surface`) plus ODE joint types `dxJointBall/DBall/Fixed/LMotor/PU/Slider/Universal` [dll RTTI].
- Log of this PC: 16 logical, 8 physical cores, "CSP uses 7 workers"; Embree build 129.5 ms (Monaco) [log-csp].

## 7. Port difficulty for rustyAC

Overall **XL** if the goal were CSP parity (script physics, COSMIC suspensions, K&C tables, double-precision core, Embree rays); **S to M** for the "always-on" behaviours of a stock car.

- **S**: `Car::isInPits` 1.5 m rule, `lockControlsUntil` caps, `PITS_ORDER` pit time, `CHANGE_TYRES_PER_AXIS`, `Car::isSleeping` (flag), `setDamageLevel` gate, `PhysicsObject::setWorldMatrix` stop, 1 ms transponder (`PRECISE_TRANSPONDER`: rustyAC has the lap timing in `docs/port/race_rules.md`; the fractional crossing is a pure function of two positions), adaptive performance meter.
- **M**: smooth controls locking, stationary auto-brake modes, `getPhysicsState` post-processing (accG low-pass 0.25 etc.: affects only exported state), air density by altitude (`Car::updateAirPressure`; the owner's recording already shows the law: `docs/map/telemetry.md` 9.5), mesh collider "sooner" + clamp (needs the stage-3 mesh colliders already ported in `docs/port/contacts.md`), track-object collider optimisation, `[EXTRA_RULES]` handling for online.
- **L-XL**: car physics scripts (LuaJIT instance on the physics thread, 992-function API, `limp_physics_thread`), COSMIC suspensions + K&C tables, CSP's own ODE build (double precision; different arithmetic means no bit-exactness with acs; prior P3), Embree tyre rays, the LLTP pool and parallel contact search.
- **Reusable from the port**: the `PhysicsEngine`/multi-car order (`docs/port/several_cars.md`), `Car::step` shell and `stepComponents` order (`docs/port/whole_car.md`, `rolling_chassis.md`), contacts (`docs/port/contacts.md`), pit stops (`docs/port/pit_stops.md`), race rules (`docs/port/race_rules.md`).
- **Proof method**: record the CSP-on game with the unpatched oracle's `car_oracle multi` scenarios (CSP can be switched off in Content Manager: `general.ini [BASIC] ENABLED=0`, prior R1) and compare per step: `framesToSleep` behaviour (AI cars freeze 0.75 s later than the player), air density (`airDensity` page field, 1.2348 vs CSP's altitude law), lap times (1 ms vs 3 ms quantisation: the 3 ms `stepCounter % 3` offset), pit time sum for a `PITS_ORDER` string, `getSteerFF` zeroing cases. A bit-exact whole-car comparison of an extended-physics car is not possible without CSP's core.

## 8. Open questions and not mapped

1. **Double registrations.** `PhysicsDriveThread::step` (0x68a1d0 / 0x68a200), `Car::pollControls` (0x59fdb0 / 0x59fe60) and `Car::getPhysicsState` (0x59dfa0 / 0x59e920) are each registered twice for one acs function (`acs_overloads` = 1), each with its own slot. Neither "step" variant calls its slot, and `pollControls` #1 would zero the controls of every car, so they cannot be chained; which variant is installed (or whether one is only for another game mode / the preview feature) is not known. I treated #2 of each as the normal path.
2. **Where `evOnStepCompleted` (`Car::postStep`) runs in the thread path.** It is explicit in `PhysicsEngine::step` 0x68b580 but not in 0x68a200; it must be inside 0x68acd0 or 0x68b330 (both not decompiled), which would also fix its order against the core step and `PARALLELIZED_CONTACTS`.
3. Contents of 0x68acd0: order of cars, who calls `stepPreCacheValues` / `Track::step` / `stepWind`, thread-local air density per car, FFB-first-car scheduling ("FFB is being sent while computing the first car" string).
4. The `N` of `VERSION=extended-N`, the `extendedV2-` family, and what each level enables; where `ext` is created and whether it exists for every car.
5. Car+0x3e9c: field written by whom; Car+0x3db4 bit writers.
6. Names of the globals marked [interp] in 2a (`DAT_182697c0f`, `_DAT_1826add30`, option bytes) were inferred from behaviour; no writer was read.
7. The "preview state" feature (`getPhysicsState` #1) and the override provider (0x59c1f0) are inferred from file names and structure only.
8. How `Car::onCollisionCallBack` (damage, collision events) is reached with `PhysicsEngine::onCollisionCallBack` disabled: presumably from CSP's own contact code; not verified.
9. Script-side semantics: which callback names besides `script.update` / `script.reset` the loader calls (inside 0x136d110 / 0x1375c60).
10. Not read: car data inside `data.acd` and the VRC scripts (not opened; standard AC container).

## Evidence

- `csp_hook_table.tsv` rows of the 18 classes + 13 `Sim::` rows; `csp_hook_table_raw.tsv` (registration sites, slots).
- Decompiles read in full: `decomp_hooks/` 5996f0, 599ec0, 59bf00, 59c0d0, 59c300, 59ce20 (almost all), 59dfa0, 59e920, 59fdb0, 59fe60, 59fff0, 5a0120, 595590, 595660, 5956a0, 595710, 595750, 595800, 595b40, 596f80, 597210, 5972b0, 59c1b0, 59dd40, 59dd80, 59f4a0, 68aa80, 68ab90, 68b580, 6865b0, 68a1d0, 68a200, 68a230, 682d70, 682d90, 682dc0, 682e80, 683080, 683130, 683160, 68d500 (first 330 lines + call list), 68f230, 426160, 5c0890, 5c0760, 5c1eb0, 6a2740, 6a2760, 6a5ab0, 6a5b40, 6a5b70, 6a5e70, 6a61c0, 6826b0, 682720, 682420, 6216c0, 2db1f0, 11bd380, 11bce70, 11be250, 11be370, 11bf830, 11bfa00; 5bba80, 563dc0, 5dbce0 as examples for other classes.
- Docs: `docs/map/car.md`, `car_step.md` (3.2-3.4, 5.1-5.8), `docs/atlas/main_loop.md` 3.5-3.6, `docs/port/several_cars.md`, `contacts.md`, `prior/csp_survey.md` (sections 3, 4, 5, 6).
- CSP files: `extension/config/general.ini` lines 128-215; `internal/lua-sdk/ac_car_cphys/{lib.lua,README.md}`, `lua_sdk_counts.tsv`, `rules.json`; `Documents/Assetto Corsa/logs/custom_shaders_patch.log` lines 120, 215, 269-271, 337, 890-891; `content_scan.json`.
- Call targets of non-decompiled functions were listed by reading the call/lea/`FF 15/25` instructions of `dwrite_copy.dll` (read-only, printed to the terminal): 0x68abe0, 0x68acd0, 0x68b330, 0x689360, 0x689070, 0x68b190, 0x599000, 0x597f00, 0x594fc0, 0x595b70, 0x597400, 0x856ed0, 0x1375740, 0x6865e0, 0x5c0920, 0x11bcf00, 0x945a70; callers of 0x918c80, 0x32c650, 0x595b70, 0x5a07c0, 0x599000, 0x597f00.
- RTTI and strings: `dll_strings.tsv` (`cphys`, `limp_physics_thread`, `extended-`, `extendedV2-`, `EXTRA_RULES` block 0x2247cf8..0x2248158, profiler strings 0x221f8a0..0x221fd50).

## Decompile requests

1. `0x68acd0` (920 B): per-car loop, LLTP dispatch, stepPreCacheValues/Track::step/stepWind, evOnStepCompleted loop, thread-local air density; answers open questions 2 and 3.
2. `0x68abe0` (41 B): which engine fields are stamped.
3. `0x68b330` (581 B): is the evOnStepCompleted loop here; collision mask rule for movable objects.
4. `0x689360` (246 B) and `0x689070` (250 B), `0x68a440` (315 B): the paused branch and the wait/oversleep logic.
5. `0x68a090` (303 B) and `0x68a380`: thread init (priority, `PHYSICS_TIMING`).
6. `0x689b00` (1,417 B) and `0x68b190` (410 B): late-frame report and "Physics is stuck waiting for ... and its physics script" watchdog.
7. `0x595b70` (2,208 B), `0x5a07e0` (336 B), `0x5a07c0`: Car constructor wrapper and how `ext` is created and attached; which `suspensions.ini` types it recognises.
8. `0x599000` (1,772 B): GUID-0 pre-step.
9. `0x597f00` (4,117 B) and `0x597400` (2,812 B): post-original step, the "cphys_data" / "ac_ier_mem" objects.
10. `0x918c80` (1,498 B) and `0x32c650` (9,705 B, `[CarSwap]` loader), `0x337280`, `0x339f80`: the `VERSION=extended-N` / `extendedV2-N` parser and the object it fills.
11. `0x945a70` (+ `0x945720`): `[EXTRA_RULES]` parser and field offsets of `_DAT_1826a9690`.
12. `0x856ed0` (2,197 B), `0x1373420` (2,819 B), `0x1375740` (1,297 B), `0x136d110` (2,263 B), `0x1375c60` (1,445 B), `0x866480` (1,265 B): script loader/runner and the actual Lua call; the callbacks invoked.
13. `0x410de0` (11,410 B, "CPHYS Controller") and `0x41f240`: the controller bridge in `ext+0x310`.
14. `0x59c1f0`, `0x11a9cb0`: the car-state override object and provider.
15. `0x59c6e0` (1,791 B): alternative collider-mesh initialiser ("Physics data is missing: %s").
16. `0x5bf9d0`, `0x5bbab0` (595 B): cphys drivetrain object and its step.
17. `0x876520`, `0x876fe0` (791 B), `0x870b00`, `0x8d8270` (1,441 B), `0x601260`: steering model, axle objects, suspension-impact helper.
18. `0x53ca50` (1,464 B, "Unknown cphys ref in stored car state"): stored car state (replay / preview).
19. `0x84f430` (7,200 B, "Creating cphys ARB") and `0x835f70` ("Cphys Drivetrain Initialized."): creation of cphys sub-objects for every car.
20. `0x7381c0`, `0xb17820` (172 B, "cphysValues"): the global value block read by the hooks.
