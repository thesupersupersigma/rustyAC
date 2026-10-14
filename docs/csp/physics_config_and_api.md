# Physics extensions: switches, data files, config keys and Lua API

Brief for `docs/csp/physics.md`. This area is everything that decides whether a car "drives right" under Custom
Shaders Patch and that CSP either **documents** (ini comments, Lua SDK, shipped Lua) or leaves **readable as text**
(key names in the DLL string pool, the data files of the installed mod cars, the owner's logs). It covers the switches
that turn extended ("custom", "cphys") physics on, the physics settings, the data-file extensions for cars and tracks,
the physics Lua API, how a car script plugs into the 3 ms step, the extra surfaces, what changes for every car even
without extended physics, and port difficulty. The two sibling briefs read the hooked machine code by class; this one
does not repeat them, but it adds a few small machine-code facts (decoded by hand from the DLL bytes) because they
change what the text means.

Markers: `[ini]` shipped ini comment, `[sdk]` Lua SDK, `[lua]` shipped Lua, `[dll]` seen in dwrite.dll (RVA given; image
base 0x180000000), `[dll-bytes]` x86-64 bytes of the DLL decoded by hand (no tool), `[log-csp]` / `[log-ac]` the owner's
logs of 2026-10-12, `[prior]` the earlier task's notes (`prior/`), `[scan]` `content_scan.json`, `[interp]` my inference.
No car `data.acd` was opened, nothing was decrypted, nothing outside this one file was written.

---

## 0. Reader's map, and what this file corrects

| Deliverable | Where |
|---|---|
| 1. every switch that turns extended physics on | section 5.1 (mechanism decoded in 2.2) |
| 2. every physics config key of `extension/config/*.ini`, counted | section 3.1 |
| 3. data-file extensions for cars and tracks, what `extended-N` means | section 3.2 |
| 4. the physics Lua API, grouped and counted, lap invalidation | section 3.3 |
| 5. how a cphys car script plugs into the step | section 3.4 |
| 6. extra surfaces, surfaces_fx, which installed tracks use them | section 3.5 |
| 7. always / needs extended physics / needs the car's script | section 5.3 |
| 8. port difficulty, per feature, with proofs | section 7 |

Corrections to `prior/csp_survey.md` and `prior/alpha2026_csp.md` (each verified below, evidence in the section named):

| # | Earlier statement | Now | Section |
|---|---|---|---|
| C1 | "`extended-N`: N changes something" (unknown) | **N is just the number `acs.exe` would have read there.** CSP hooks `INIReader::getInt` / `getFloat`, strips the prefix, flags the car or track as extended and returns N. `VERSION=extended-2` is `VERSION=2` plus the flag; `WAV_PITCH=extended-0` is `WAV_PITCH=0` plus the flag. A second prefix `extendedV2-` exists for cars. | 2.2 |
| C2 | the plain exe stops at `std::stoi` on `extended-2` | still true for plain `acs.exe`; under CSP that exact call (`INIReader::getInt`, 0x140235c70) is one of the hooks that makes the value parse | 2.2 |
| C3 | `content_scan.json` `surfaces_extended` empty, "no installed track uses extended surfaces" | the scan tested key `WAV`, not `WAV_PITCH`. A direct read finds **6 installed track layouts** with `WAV_PITCH=extended-0` and `[_SCRIPTING_PHYSICS] ALLOW_APPS=1` (all "fa_2026" layouts) | 3.5 |
| C4 | air density law "fitted; humidity guess"; "unknown whether the engine sees it" | the law is **decoded**: barometric formula, 379 bytes at RVA 0xaf7760, constants 273.15, 0.0065, 5.2557769, 101.325, 3.4836106. It reproduces the owner's recording to 0.03 K. The engine **does** see it (thread-local override set in the `Engine::step` hook). | 2.4, 5.3 |
| C5 | `[PHYSICS_EXPERIMENTS]` hint "offline races only" is a hint | it is enforced in code: the whole experiments block is skipped online, with a special event, with the SIMRACERS or RSRLIVETIME app and with the hidden key `FORCE_BASE_PHYSICS` | 2.4 |
| C6 | "several `[PHYSICS_EXPERIMENTS_DONE]` keys have no description" | still true; but the DLL also reads keys that are in no shipped ini: `FORCE_BASE_PHYSICS`, `INTEL_EMBREE_ROBUST`, `LIMIT_CONTACTS`, `ICE_EVERYWHERE`, `ALLOW_CPHYS_SCRIPT` ... (3.1.9) | 3.1 |
| C7 | a car script has "functions such as `physics.*`" | a **car** script gets only `physics.allowed()` and `physics.raycastTrack()` of the `physics` table; its physics API is the `ac.*` set (95 own functions) plus `ac.accessCarPhysics()`. The 107-function `physics.*` API belongs to track / app / tool / online / new-mode scripts. | 3.3 |
| C8 | `[_SCRIPTING_PHYSICS]` has 4 keys (`ALLOW_TRACK_SCRIPTS`, `..._DISPLAY_SCRIPTS`, `..._NEW_MODE_SCRIPTS`, `ALLOW_TOOLS`) | the DLL knows **8**: it also has `ALLOW_APPS` (used by the 6 installed layouts), `ALLOW_CUSTOM_AI_MANIPULATION`, `ALLOW_TRACK_SCRIPTS_STATIC_OBJECTS`, `ALLOW_TRACK_SCRIPTS_DYNAMIC_OBJECTS`, and a section `_EXTRA_PERMISSIONS` | 3.5 |
| C9 | `ac_car_cphys` "has the Lua physics API with RigidBody" (SDK README skim) | `physics.RigidBody` and `physics.addTrackSurface` are **not** in the car SDK; they exist in the track / apps / tools / online / new-mode SDKs and "require the double precision physics engine" | 3.3 |

---

## 1. What it does

CSP does three different things to physics. They are easy to confuse, so keep them apart.

1. **Changes that apply to every car, extended physics or not** (section 5.3). The air density law, the ODE core
   (CSP's own double-precision build with libccd and multithreading), the thread pool, a set of "enabled everywhere
   now" collision and brake fixes, surface-noise changes, a virtual rain compound, and per-car tweaks that the
   launcher can switch (stalling, awake-in-pits, auto-braking). The owner's installation has none of the optional ones
   changed.
2. **Extended physics** (a.k.a. cphys, "custom physics"). A car or a track says so by writing `extended-N` where a number
   is expected. A car then may use data CSP adds to the car files (COSMIC suspension, aero maps, dampers/bump-stop
   tables, thermal tyre model, brakes with hysteresis, extra differential and drivetrain damage keys, script inputs,
   extra setup items) and a Lua script `data/script.lua` that runs on the physics thread and can add forces, replace
   engine / brake / clutch / differential behaviour and feed `SCRIPT_n` controller inputs. A track may allow Lua
   scripts to use the `physics.*` API, add rigid bodies and surfaces, and teleport cars.
3. **Offline experiments** (`general.ini [PHYSICS_EXPERIMENTS]`, `[OTHER_EXPERIMENTS]`, `[PHYSICS_EXPERIMENTS_DONE]`).
   Optional (or already-always-on) fixes to collision handling, contact-point search, track-object colliders, stalling,
   pits behaviour. They are **offline only** (2.4).

Whether a car "drives right" is decided by: (a) the data of the car being loadable under CSP's loaders (the
`_csp` VRC cars are not loadable by plain AC at all), (b) the car's encrypted Lua script, and (c) CSP's own physics code,
which is closed. Everything in (c) that is text is in this file.

On this PC (installed content, `content_scan.json`): **204 cars, 26 tracks**; 4 cars use CSP's COSMIC suspension or
DWB2 (the three VRC `_csp` formula cars use `COSMIC`; `vrc_formula_lithium_2023_csp` is `DWB` with `USE_DWB2=1` and a
script), 1 more car carries a CSP script (`urd_darche_992_23`, data protected by its maker), and 6 track layouts allow
extended physics. Everything else runs the stock data on CSP's always-on changes.

---

## 2. Game parts it replaces or extends

### 2.1 Counts

| What | Number | Evidence |
|---|---|---|
| hook records on `ksPhysicsAC.lib` | **155 hooks + 82 lookups** = 237, in **51 classes** (154 distinct replacement functions) | `csp_hook_table.tsv` [dll] |
| hook records on `ksPhysicsCoreODE.lib` | 2 hooks (`ksPhysicsCoreODEFactory::create`, `CollisionMeshODE::CollisionMeshODE`) + 1 lookup | same |
| `ksTyreModel.lib` | 1 lookup (`BrushSlipProvider::recomputeMaximum`) | same |
| related hooks outside those libs | `INIReader::getInt` / `getFloat` / `getString` / `parse` (+7 more `INIReader` hooks), `SurfacesManager::loadSurfaceDefinitions` / `getSurface`, `TrackAvatar::initPhysics`, `CarAvatar::initPhysics`, `PhysicsAvatar::startPhysics` / `update` / `stepCommandQueue`, `PhysicsDriveThread::step` / `run` | same |

### 2.2 The switch mechanism (decoded)

| Switch | acs.exe function (VA) | CSP replacement (RVA) | What it does | Evidence |
|---|---|---|---|---|
| car `[HEADER] VERSION` | `INIReader::getInt` 0x140235c70 | 0x5f0650 | When the reader asks for section `HEADER`, key `VERSION`: reads the text with the original `getString`; if it starts with `extended-` (9 chars) or `extendedV2-` (11 chars) it calls a flag setter (RVA 0x5f0170) on the object of the car being loaded (global pointer at RVA 0x26a9d50) and parses **the number after the prefix** (the string after the prefix is passed to a number parser; that its result is what the caller receives as the integer is [interp], the decompile does not show the return register). If the global byte at RVA 0x26998bd (= `EXTENDED_PHYSICS`) is set, the flag setter is called for any value. | [dll] decomp `decomp_hooks/5f0650.c` |
| track `[SURFACE_n] WAV_PITCH` | `INIReader::getFloat` 0x1402358c0 | 0x5f1110 | When the reader asks for key `WAV_PITCH` in a section whose name starts `SURFACE_` of a file whose path ends with the 18 characters `/data/surfaces.ini` (UTF-16 constant at RVA 0x221c9c8; it only runs while the track avatar's physics is not yet created): reads the text; if it starts with `extended-` calls a flag setter (RVA 0x9471a0, the "track is extended" flag) and parses the number after the prefix as the float value. Only `extended-` is accepted here (no `extendedV2-`). | [dll] `decomp_hooks/5f1110.c` |
| per-car "is extended" bit | (CSP's own car loader, RVA 0x32c650, 9,705 bytes) | not a hook | Writes one byte at car-object offset +0x914: true if the global `EXTENDED_PHYSICS` byte is set **or** `VERSION` starts with `extended-` / `extendedV2-`. The same test exists as a stand-alone predicate at RVA 0x918c80 (1,498 bytes). | [dll-bytes] 0x32ca85..0x32cab8, 0x918c80 |
| `EXTENDED_PHYSICS` (general.ini) | n.a. | settings loader 0x138ded0 (18,589 bytes) | Reads `[PHYSICS_EXPERIMENTS] EXTENDED_PHYSICS`; when true stores 1 in the global byte at RVA 0x26998bd and calls one more setter. Only runs when the experiments gate (2.4) is open. | [dll-bytes] 0x138f8c2, 0x138f919 |
| car `[CONTROLS] FFMULT`, `STEER_ASSIST` | `INIReader::getFloat` | 0x5f1110 | When `general.ini [INPUTS_REMAP] OVERRIDE_CAR_FFMULT` / `OVERRIDE_CAR_STEERASSIST` are non-zero, the car.ini value is replaced by them at read time (they are *not* "no effect on physics": steer assist is read by the physics step). | [dll] same decomp, `[ini]` section INPUTS_REMAP |

Consequences:
- **`extended-N`: N is the original number.** The plain VRC Formula Alpha 2026 has `VERSION=2` and the `_csp` one
  `extended-2` [prior]; plain Spa has `WAV_PITCH=0` and `spa_fa_2026` has `extended-0` (diff of the two files, the only
  other difference is the added `[_SCRIPTING_PHYSICS]` section). Nothing in the decoded hooks gives N a physics meaning.
  What `V2` means in `extendedV2-` is **not** visible: both prefixes follow the same path in both places I decoded
  [interp: a second generation of the script / physics API].
- Plain `acs.exe` has no such hook, so `extended-2` reaches `std::stoi` and throws (prior 4.1 stop 1).
- Because the flag setter is called on the object of the car *being loaded*, the flag is per car; the track flag is
  per session. [interp]

### 2.3 Hooked physics classes (ksPhysicsAC.lib, ksPhysicsCoreODE.lib)

Format `member acs.exe-VA-low-6-hex <CSP hook RVA>`; `*` = lookup only (no replacement). Class order by hook count.
Replacement RVA 0x2db1f0 is shared by two members and is a stub [interp].

| Class (hooks / lookups) | Members |
|---|---|
| `Car` (26 / 7) | isInPits 274530<595590>; setControllerProvider 275a70<595660>; resetSuspensionDamageLevel 275970<5956a0>; isSleeping 2745e0<595710>; getCGHeight 2703b0<595750>; getSteerFF 272180<595800>; Car 26bf00<595b40>; **updateAirPressure 276ae0<596f80>**; getGroundWindVector 270c00<597210>; computeRideHeight 26fc90<5972b0>; **step 275da0<5996f0>**; **stepComponents 2764d0<599ec0>**; forcePosition 26fe10<59bf00>; reset 2758e0<59c0d0>; setDamageLevel 275b20<59c1b0>; updateColliderStatus 276df0<59c300>; initColliderMesh 273b20<59ce20>; addPenalty 26f6a0<59dd40>; setBlackFlag 2759f0<59dd80>; getPhysicsState 270d70<59dfa0, 59e920>; lockControlsUntil 274600<59f4a0>; pollControls 274e70<59fdb0, 59fe60>; getWingState 272a20<59fff0>; getPitstopTime 271e60<5a0120>. Lookups: getTotalMass, isInPitLane, onTyresStepCompleted, onCollisionCallBack, forceRotation, setRestrictor, stepPreCacheValues |
| `Tyre` (17 / 7) | Tyre 26dbd0<6a9b30>; ~Tyre 27d320<2db1f0>; initCompounds 280800<6a9fc0>; updateAngularSpeed 284fa0<6aa310>; reset 283380<6aa5a0>; getCorrectedD 280190<6aab80>; updateLockedState 285070<6abfc0>; **step 283800<6ac2a0>** (2,544 lines); stepDirtyLevel 2843d0<6b22e0>; stepThermalModel 284ca0<6b2640>; stepTyreBlankets 284f10<6b2ce0>; addGroundContact 27d980<6b2dc0>; **addTyreForcesV10 27ed60<6b40e0>**; addTyreForceToHub 27dc00<6b68b0>; getDY 2803d0<6b68c0>; getDX 280240<6b6a00>; setCompound 2834e0<6b6b40>. Lookups: getDynamicK, stepRotationMatrix, stepRelaxationLength, stepGrainBlister, stepFlatSpot, stepPuncture, addTyreForces |
| `AIDriver` (12 / 9) | AIDriver 298c60<107e590>; acquireControls 29baa0<107f280>; updateCurrentSplinePos 2a3200<1082bf0>; stepGasBrake 2a08e0<1082f60>; stepGasBrakeStart 2a1580<1083130>; stepTurbo 2a30c0<10832f0>; stepGears 2a1710<1083310>; stepCarsAwareness2 29f8d0<1083950>; getLABraking 29d650<1083ad0>; stepSuperhuman 2a2a60<10847e0>; stepSteer 2a2630<1084800>; getSteerAtDistance 29de70<1085090>. Lookups: initSetup, generatePitlaneWait, stepSplineSelection, getCornerRadiusHint, getLateralPush, updateTyres, onNewSession, stepSetup, findTyreForQualify |
| `RaceEngineer` (11 / 2) | getMaxSpeedFromGear 27c240<10eb610>; **getOptimalBrake 27c320<10eb630>**; getCasterRAD 27bb20<10eb880>; evalFrontRideHeight 27a0f0<10ebb50>; evalRearRideHeight 27a850<10ebcf0>; getPointGroundHeight 27c550<10ebeb0>; evalLateralGrip 27a510<10ebff0>; evalLateralGFromLoads 27a260<10ec0e0>; findTyreCompound 27b3a0<10ec5e0>; projectWingsDrag 27ccf0<10ec740>; projectWingsLift 27cdc0<10ec800>. Lookups: RaceEngineer, getWeightDistribution |
| `AISplineRecorder` (7 / 4) | load 2952c0<58a790>; loadHints 295670<58a7c0>; AISplineRecorder 293bf0<58a7f0>; startRecording 296f00<58a810>; save 296ae0<58a820>; ~AISplineRecorder 293d20<58a940>; step 296f60<58aa10> |
| `TyreThermalModel` (8 / 2) | getCorrectedD 2adbc0<6b8fc0>; getCurrentCPTemp 2adbe0<6b9070>; TyreThermalModel 26df80<6b93a0>; getPracticalTemp 2ae140<6b94a0>; step 2ae340<6b98d0>; init 2ae170<6b9d00>; reset 2ae1c0<6b9fa0>; addThermalInput 2ad090<6ba0f0> |
| `Drivetrain` (6 / 3) | step 26b130<5bba80>; getCurrentGear 266b10<5bbd10>; step2WD 2694e0<5bd2b0>; step4WD 26a220<5be440>; step4WD_new 26ad80<5bf3f0>; isGearboxLocked 2673c0<5bf970> |
| `Engine` (4 / 5) | loadINI 286100<5cacb0>; getSafeTurboLevel 285b50<5cae10>; getThrottleResponseGas 285b90<5cae40>; **step 2880e0<5cbdc0>**. Lookups: setCoastSettings, getMaxTurboBoost, getTorqueAtRPM, stepP2P, stepTurbos |
| `PhysicsEngine` (4 / 5) | PhysicsEngine 262430<68aa80>; **getAirDensity 263a60<68ab90>**; **step 264760<68b580>**; onCollisionCallBack 264020<2db1f0>. Lookups: stepPaused, stepWind, getCore, isTyreLegal, initLowSpeedFF |
| `TimeTransponder` (5 / 3), `AISpline` (5 / 2), `Suspension` (5 / 1), `SuspensionStrut` (4 / 1), `Wing` (4 / 2), `DynamicController` (3 / 1), `SetupManager` (3 / 1), `SuspensionAxle` (3 / 0), `SuspensionML` (3 / 0), `Turbo` (3 / 0) | lap, init, onTimeLinePassed, step, reset; payloadAtPosition, loadFast, calculateNormals, initStraights, load; Suspension ctor, getHubWorldMatrix, attach, setDamage, addForceAtPos (step is a lookup); strut ctor / attach / setDamage / step; Wing ctor x2, step, addLift (addDrag, stepDynamicControllers are lookups); DynamicController ~ / ctor / getInput; SetupManager init 289290<10f4e00>, step, load; axle ctor / attach / step; multilink ctor / attach / setDamage; Turbo ctor / step / getBoost |
| two-member classes | `BrakeSystem` step 28e640<108eef0>, reset (stepTemps lookup); `PerformanceMeter` reset, step; `ABS` step 28f610<563dc0>, isInAction; `DynamicWingController` initCommon, getInput; `PhysicsObject` ctor, setWorldMatrix; `SpeedLimiter` init, step; `CarColliderManager` loadINI 2a37a0<426160> (step lookup); `PenaltyManager` step (addJumpStartPenalty lookup) |
| single hooks | `AntirollBar::step`, `CollisionMeshODE` ctor, `DRS::step` 2b4e60<426480>, `DRSManager::isDRSAvailable`, `Damper::getForce`, `FuelLapEvaluator::step`, `HeaveSpring::step`, `LapInvalidator::step`, `TractionControl::step` 290200<6a7cd0>, **`ksPhysicsCoreODEFactory::create` 2cb630<6216c0>** |
| lookups only | `AeroMap::step` / `getCurrentLiftKG` / `getCurrentDragKG`, `ERS::step` / `setPowerController`, `Kers::step`, `AutoBlip::step`, `AutoShifter::step`, `StabilityControl::step`, `EDL::step`, `SteeringSystem::step`, `DriftModeComponent::step`, `Telemetry::step`, `SCTM::solve`, `PhysicsCore::onCollision`, `SlipStream::getSlipEffect`, `SplineLocator::*`, `Track::step` / `addSurface` / `initAISpline`, `SetupItem::SetupItem`, `DynamicControllerStage` ctor |

What the odd shape tells: the ERS, KERS, aero map, auto-shifter, stability control and EDL **are not hooked** (lookups
only). CSP's cphys ERS is therefore a *separate* implementation reached through its own objects (the log line "CPHYS
ERS Initialized" / "not enabled; initialization skipped", RVA 0x2202ce0 / 0x2202d10 in function 0x441c80, which reads
`[FRONT_MOTORS]`, `[KINETIC]` and a `[_EXTENSION]` section) and the stock `ERS::step` stays as the display channel
[interp, matches prior 3.8]. The aero map (`AeroMap::step`) is looked up, not replaced; extended aero maps are applied
through the hooked `Wing::step` / `Wing::addLift` and `Car::stepComponents` [interp].

### 2.4 Decoded pieces that change what the text means

**(a) Air density, aero side: `Car::updateAirPressure` replaced** (RVA 0x596f80, decompiled) [dll].
- Stock: `rho = 1.2922 - ambientTemperature * 0.0041` (`docs/map/aero.md` 5.2).
- CSP: base density is taken from CSP function **RVA 0xaf7760** called with (conditions at car-avatar +0x418, avatar,
  flag 1) when the car's avatar has the conditions object (avatar +0x228 non-null); otherwise the stock formula with
  the engine's ambient temperature (default 20 C when there is no PhysicsEngine).
- Then multiplied by a per-car factor read from a 256-float table at RVA 0x25e1a30 indexed by the car's slot
  (`car+0x3238`, < 0x100, default 1.0) [interp: this is `physics.setAirDensityMultiplier`].
- Slipstream is the stock rule (`rho * (1 - 0.25 * s)` with gain 1); CSP additionally keeps a low-passed copy
  (time constant 0.015 per step) of the strongest slipstream strength at car +0xaec (for audio, [interp]).

**(b) The law itself** (RVA 0xaf7760, 379 bytes, hand-decoded) [dll-bytes]:

```
h   = isnan(B) ? 0 : B + car_position_y          B = float global (RVA 0x268c430 with flag, 0x268c48c without) = base altitude
Tc  = Z(roadTemp, ambientTemp, |PhysicsEngine+0x164|, groundHeight)   Z at RVA 0xaf7010 (not read); groundHeight from
                                                                      RaceEngineer::getPointGroundHeight hook (0.5 m default)
TK  = Tc + 273.15
x   = max(1 - 0.0065 * h / TK, 0)
P   = max(101.325 * x ^ 5.2557769, 1.19e-7)      kPa, standard barometric formula with the local temperature
rho = 3.4836106 * P / TK                          kg/m3 (3.4836 = 1 / 0.287058)
```

Check against the owner's recording [prior data, `f2004_spa_ai.csv`]: with B = 395.011 m (= pit altitude 418 m -
pit Y 22.9886 m, from [log-csp] of that session) the formula reproduces the five measured points (posY 0 / 5.3 / 14.6 /
73.7 / -28.7: 1.16766 / 1.16714 / 1.16580 / 1.15750 / 1.17172) with a **constant effective temperature of 15.2 to
15.27 C**, i.e. 1.2 K above the recorded `airTemp` 14.0 (spread 0.06 K = 0.02 % in density). The local scale height
at that temperature is 8,442 m (the earlier fit gave 8,365 to 8,415). The 1.2 K offset is what `Z` adds: 14 + 0.2 *
(road 20 - air 14) = 15.2 fits [interp, the 0.2 factor is a guess; Z not read]. The earlier "relative humidity about
66 %" explanation is **not needed**.

**(c) Engine side** (`Engine::step` hook, RVA 0x5cbdc0, decompiled) [dll]. Before calling the original it writes a
thread-local float (TLS block offset 0x170, sentinel FLT_MAX = "unset") = `f(...0x101) * per-car factor`, where f is
the same function 0xaf7760 (low flag byte 1), or a constant **1.1839 kg/m3** when a flag byte (+0x6cf of the object
behind avatar +0x228) is set. The hooked `PhysicsEngine::getAirDensity` (RVA 0x68ab90) returns that thread-local value
when it is not the sentinel, else the stock `1.2922 - T * 0.0041`. The stock `Engine::step` uses `rho * 0.8263`
(`docs/map/aero.md` 569-570). So **the engine sees CSP's altitude-dependent density for every car** with a conditions
object, extended physics or not (answers prior UNCONFIRMED 2). Magnitude: ~5.6 % less air at Spa (prior 5.1).

**(d) ODE core** (`ksPhysicsCoreODEFactory::create`, RVA 0x6216c0, decompiled) [dll]: not a thin wrapper; it
allocates a 0x32c0-byte object (CSP's own core, class names `cphys::ode64_core`, `ode64_rigidbody`, `ode64_dynamic_collider`,
`ode_impl_*` joints in RTTI [dll .data 0x25f9330..0x261ab40]), copies four settings bytes from the settings block at RVA
0x2695087 and 0x26950a3..0x26950a5 (the last clamped to 12 = worker threads [interp]) and stores the new core in the
engine's slot. [log-csp] prints `ODE_double_precision`, `ODE_EXT_libccd`, `ODE_EXT_mt_collisions`, `ODE_EXT_threading`. So the double
precision engine is the world **every** car lives in, not only extended ones. Timing strings: "ODE64: world step",
"ODE64: collision step (dynamic/static, v1/v2)" [dll 0x2246ee0..0x2247050].

**(e) `PhysicsEngine::step` replaced** (RVA 0x68b580, decompiled) [dll]: calls CSP functions 0x68abe0 and 0x68acd0 (the
real step, not read), runs the engine's step listeners, then for every car toggles a collision category mask (0x1f
when the car is "collidable", 0xc otherwise) [interp]. Whether the original is called inside 0x68acd0 is not visible.

**(f) Experiments gate** [dll-bytes 0x138c028..0x138c20d, 0x138f89a..0x138f8bb]. A global byte at RVA 0x26950aa is set to
1 at the start of settings loading and cleared, with a log line, by each of: the session being online (a session
object byte; "Experimental features are not available online", RVA 0x22aa228), a special event (RVA 0x22aa258), the
app `SIMRACERS` (0x22aa308), the hidden key `FORCE_BASE_PHYSICS` read from the session's configuration (its log
line is "...not available with given session settings", 0x22aa380; the lea of the key name at 0x138c105 is followed by
that message at 0x138c175) and the app `RSRLIVETIME` (0x22aa3d8). The block that reads `[PHYSICS_EXPERIMENTS]` / `EXTENDED_PHYSICS` /
`[OTHER_EXPERIMENTS]` runs only if two byte flags of the session object (+0x11 and +0x16) are zero **and** that global
is 1. Online play therefore always runs the "base physics" of CSP (always-on parts only) unless the server asks
for extended physics by other means (the session-information strings at RVA 0x2249cd8..0x2249f40 show "Extended physics:
cars / track / cars & track", "Rain physics: disabled", "Ice physics: enabled", "Surfaces FX: enabled", "Auto-braking:
disabled", "Forced speed limiter in pit lane: disabled", "Brakes temperature in pits: retained", "Camber and toe:
synced", function 0x97fb20) [interp for the server side].

---

## 3. Config keys and Lua API

### 3.1 Physics config keys of `extension/config/*.ini`

Scope: keys whose value reaches the simulated car, track, AI, controls-into-physics, session rules or the physics /
shared-memory threads. Visual-only modules (`tyres_fx.ini`, `lighting_fx.ini`, `particles_fx.ini`, ...) are not
listed; `tyres_fx.ini` has **0** physics keys (its tyre "flat spots / wear" are visual only). 57 ini files,
4,630 key lines, 198 commented-out keys in total; **170 physics-related keys** are listed below.

"Ext?" = needs extended physics: **Y** text says so, **N** text says stock cars are affected, **H** the section hint
("Affects offline races only, and only cars with custom physics active") is all there is, **-** not applicable. The
whole `[PHYSICS_EXPERIMENTS]` block is offline-only (2.4f).

#### 3.1.1 `general.ini [PHYSICS_EXPERIMENTS]` (26 keys; 4 hidden)

| Key | Default | What it does (shipped comment, my words) | Ext? |
|---|---|---|---|
| `EXTENDED_PHYSICS` | 0 | use extended physics for all cars and tracks (master switch, 2.2) | - |
| `STALLING` | 0 | allow engines to stall (very basic; for cars that lack their own physics scripts; assign a button or use the throttle); damaged engines may be hard to start. A Lua plugin `stalling.luac` ships for it [dll overlay name] | N |
| `REPAIR_WHEELS2` | 0 | keep suspensions whole (stops DWB and strut suspensions from collapsing) | N |
| `ENABLE_MESH_COLLIDER_SOONER` | 1 | turn on the car's mesh collider earlier ("proper car-ground collisions when not driving straight"); may cost performance and break mods; can change live | H |
| `CLAMP_MESH_COLLIDERS` | 1 | mesh collider may not go below the box colliders; only with the previous key | H |
| `EXTEND_COLLIDER_BOXES` | 1 | extend car box colliders (stops cars falling through the ground when falling fast) | H |
| `DISABLE_AUTOBRAKING` | 0 | stationary behaviour: 0 original (freezes soon after stop), 1 realistic (use handbrake), 2 adapted, 3 auto-braking | N |
| `AWAKE_IN_PITS` | 2 | keep physics awake in pits: 0 off, 1 awake, 2 fully dynamic ("disables tyres grip entirely while holding the car in place") | N |
| `EXTRA_CONTACT_POINTS` | 1 | extra lateral tyre contact points for extended-physics cars that do not define them ("improves handling on curbs"; contact point no longer shifts laterally) | Y |
| `PHYSICS_SHADOWS` | 0 | road temperature drops in sun shadows (offline; online a server option) | N |
| `POSTPONE_WALLS_INIT` | 1 | loading only: wait for horizontal surfaces, let walls load later | - |
| `ALT_DEPTH_FOR_CAR_BOX_VS_TRACK` | 0 (hidden) | undocumented | ? |
| `TOE_ROD_THING` | 0 (hidden) | undocumented | ? |
| `DIFF_THING` | 0 (hidden) | undocumented (the dll has `DIFF_FRICTION_FACES`, `DIFF_RAMP_COAST`, `DIFF_RAMP_POWER`, `DIFF_PRELOAD_EXT`, RVA 0x2243cb0..0x2243d08, function 0x839d70) [interp] | ? |
| `PRECISE_TRANSPONDER` | 1 | lap times with 1 ms precision (timing, `TimeTransponder` hooks) | N |
| `ARM_FIRST_LAP` | 1 (hidden) | count the first lap on hot-lap spawns in practice too | N |
| `OPTIMIZE_COLLIDERS` | 1 | simplify colliders of track physics objects (log: "Track object collider optimized from N to M triangles") | H |
| `TWEAK_TRACK_OBJECTS` | 1 | with extended **track** physics, objects without explicit config are tweaked to behave better | Y (track) |
| `EQUALIZE_PITS_CHECK` | 1 | same threshold for the pits check everywhere | N |
| `HASH_SPACE_TRACK` | 1 | broad phase for cars vs track: 0 off, 1 hash space, 2 sparse grid | - |
| `HASH_SPACE_CARS` | 1 | hash space for cars vs cars | - |
| `ISOLATE_CARS` | 0 | collision resolution islands per car (performance when many bodies touch) | - |
| `WHEEL_CAR_CONTACTS` | 0 | wheel vs car collisions, "very experimental" | H |
| `BODY_COLLIDERS_CONVEX` | 0 | if a collider mesh is convex use better algorithms (log: "Convex collider: N points") | H |
| `ODE_BATCH_RAYCAST` | 0 | 1 = use ODE for contact-point rays; default 0 = Intel Embree | ? |
| `ONE_SIDED_BOXES` | 0 | one-sided boxes for car colliders (cleaner ground contacts) | H |

(The `__HINT_BOTTOM` line is not a key.) Hidden keys the code reads that are in **no** shipped ini: `INTEL_EMBREE_ROBUST`
(ray-triangle edge robustness, was in 0.2.11 / preview212 as a commented line), `LIMIT_CONTACTS`, `FORCE_BASE_PHYSICS`
(all in settings loader 0x1387350, strings RVA 0x22a9960 / 0x22a99e8 / 0x22aa298; section unknown [interp]).

#### 3.1.2 `general.ini [OTHER_EXPERIMENTS]` (12 keys: 6 physics or input, 1 visual extrapolation, 5 unrelated; counted below as 7)

| Key | Default | Meaning | Physics? |
|---|---|---|---|
| `MT_ODE_STEP` | 1 | parallel joints solver | yes |
| `NO_FULL_PRIM_BOX_TEST` | 1 | "collision optimization A": ODE tweak to skip full primitive-box tests | yes |
| `TRIMESH_OBB_PREFILTER` | 1 | "B": check box-vs-trimesh before trimesh-vs-trimesh | yes |
| `TRIMESH_AABB_TEST` | 8 | "C": AABB-vs-sub-AABB prefilter (8 or 0) | yes |
| `CUSTOM_FFB_INIT` | 0 | custom FFB handling | input |
| `ADAPTIVE_REMOTE_FACTOR` | 0 | smoothing for remote cars (online) | remote |
| `EXTRAPOLATE_STATE` | 4 | car state extrapolation for visuals, "doesn't affect physics" | no |
| `HIGHBEAMS_FLASHING`, `IMPROVED_MODIFIERS_HANDLING`, `ALT_LOGGER`, `ADAPTIVE_PERF_METER`, `NEW_GHOST_RECORDING` | 1 | not physics | no |

#### 3.1.3 `general.ini [PHYSICS_EXPERIMENTS_DONE]` (20 keys; 14 hidden, 6 visible) "enabled everywhere now"

| Key | Default | Description in the ini | Ext? |
|---|---|---|---|
| `SLIDING_FIX`, `BRAKES_LIMIT`, `OPTIMIZE_ROTATION`, `SMOOTH_CONTROLS_LOCKING`, `RANDOMIZE_WHEEL_ANGLES`, `CUSTOM_POBJECTS_MOTION` | 1 (hidden) | none | N ("everywhere") |
| `RIM_COLLISION`, `SMOOTH_COLLISIONS`, `ADAPTIVE_HARD_COLLISIONS` | 1 (hidden) | none | N |
| `ODE_AFTER_GRAPHICS` | 1 (hidden) | none (run ODE after graphics submission) | N |
| `ALTERNATIVE_OPTIMAL_BRAKE` | 1 (hidden) | none; hooks `RaceEngineer::getOptimalBrake` (AI / pit limiter / keyboard braking target) | N |
| `FIX_GROUND_COLLIDERS` | 1 (hidden) | prevents cars falling through ground; cannot change live | N |
| `FIX_SURFACE_TYPES` | 1 (hidden) | meshes named without "more digits" are not walls any more | N |
| `FIX_SPAWN_POINTS` | 1 (hidden) | create missing spawn points ("Creating a missing spawn point `AC_HOTLAP_START_n`") | N |
| `TYRE_SHIFT_SURFACE_PROPERLY` | 1 | alternative noise application on sinusoidal off-track surfaces | N |
| `TYRE_SINE_ALTERS_NORMALS` | 1 | sine noise also alters normals | N |
| `TYRE_PERLIN_SURFACE_NOISE` | 1 | Perlin noise instead of sinusoidal, dirt-additive surfaces only | N |
| `ALT_COLLISIONS_HANDLING` | 1 | new collision handling ("pretty much the same") | N |
| `ALT_COLLISIONS_PROCESSING` | 1 | new collision check logic (smoother, more accurate) | N |
| `PARALLELIZED_CONTACTS` | 1 | contacts search in parallel with cars' physics | N |

#### 3.1.4 other `general.ini` sections

| Section | Keys (default) | Meaning | Count |
|---|---|---|---|
| `[PHYSICS_SHADOWS]` | `TEMPERATURE_MULT` (1, hidden) | multiplier for the shadowed-road temperature | 1 |
| `[STEERING_EXTRAPOLATION]` | `PHYSICS_LEAD_TIME` (0 ms), `GRAPHICS_LEAD_FRAMES` (0), `RATE_SMOOTH_HZ` (250), `REVERSE_FADE_TIME` (10), `MAX_RATE` (12, hidden) | compensate controller lag for physics and display lag for visuals | 5 |
| `[HOLDING_HANDBRAKE]` | `ENABLED` (0), `DELAY` (3 s), `SPEED_THRESHOLD` (0.5 km/h), `AUTO_DISENGAGE` (0) | keep handbrake engaged after standing still | 4 |
| `[INPUTS_REMAP]` | `OVERRIDE_CAR_FFMULT`, `OVERRIDE_CAR_STEERASSIST`, `SKIP_THROTTLE_CURVES` (all 0, hidden) | replace car.ini `FFMULT` / `STEER_ASSIST`; cancel `throttle.lut` unless `throttle_max.lut` exists | 3 |
| `[LAP_TIMES]` | `KEEP_AI_LAPS` (0), `KEEP_ALTERED_LAPS` (0) | keep laps set by autopilot / with altered physics | 2 |
| `[SHARED_MEMORY]` | `REDUCE_GFORCES_WHEN_SLOW` (0) | reduce G below 6 km/h (D-BOX) | 1 |
| `[THREAD_TWEAKS]` | `PHYSICS_TIMING` (4 = high-resolution timer), `MMCSS_PRIORITY_PHYSICS` (1), `PROCESS_PRIORITY` (1) (+3 render keys) | how the 3 ms physics loop is paced | 3 |
| `[OPTIMIZATIONS_CPU]` | `HYPERTHREADING_FIX` (0) | physics worker per physical core (-1 off, 0 limit workers, 1 separate render / physics) | 1 |
| `[CONTROL]` | `ALTERNATIVE_AI_DRIVER` (1), `AI_DRIVER_KEEP_SETTINGS` (1) | autopilot switching; stop autopilot altering car settings | 2 |
| `[NEW_KN5_LOADER]` | `STRIP_PHYSICS_DATA` (1) | strip unneeded data from physics meshes | 1 |

`general.ini` physics-related total: 26 + 7 + 20 + 1 + 5 + 4 + 3 + 2 + 1 + 3 + 1 + 2 + 1 = **76**.

#### 3.1.5 `ffb_tweaks.ini` (24 keys)

| Section | Keys (default) | Meaning | Ext? |
|---|---|---|---|
| `[BASIC]` | `ENABLED` (1) | allow CSP to alter FFB | - |
| `[CUSTOM_SOFT_LOCK]` | `ENABLED` (1), `PADDING` (10 deg), `SHIFT_PADDING` (0.8), `FORCE_FACTOR` (1), `SPEED_FACTOR` (1), `DAMPER` (0) | smoother, less bouncy steering lock | N |
| `[POSTPROCESSING]` | `RANGE_COMPRESSION` (1.0), `RANGE_COMPRESSION_ASSIST` (0), `CLEAN_FFB` (1), `DIRT_VIBRATION` (0.0) | extra FFB post-processing; `CLEAN_FFB` removes duplicated post-processing from cars with custom physics | `CLEAN_FFB`: Y |
| `[POSTPROCESSING_SCRIPT]` | `ENABLED` (0), `IMPLEMENTATION` (alternative) | Lua FFB post-process (`extension/lua/ffb-postprocess`) | N |
| `[REAL_FEEL]` | `ENABLED` (0), `WHEEL_MAX_NM` (35.0), `ENABLE_FOR_ALL` (0) | real steering forces to the wheel for cars "that include the required physics code"; `ENABLE_FOR_ALL` forces it on cars without extended physics | Y (`ENABLE_FOR_ALL` overrides) |
| `[REAL_FEEL_BRAKE]` | `ENABLED` (0), `BRAKE_MAX_KG` (0.0), `BRAKE_MAX_INACTIVE_PERC` (100.0), `CLAMP_FORCES` (1) | real brake pedal force (load-cell pedals) for cars with the data | Y |
| `[GYRO2]` | `ENABLED` (0) | more physically accurate gyro; "doesn't affect cars using extended physics" | N |
| `[TWEAKS]` | `ADAPTIVE_FFB` (0), `AVERAGE_SKIPPED_FFB` (1), `SKIP_DAMPING_FRAMES` (3) | keep FFB from slowing physics | N |

#### 3.1.6 `surfaces_fx.ini` (11 keys)

| Section | Keys (default) | Meaning | Ext? |
|---|---|---|---|
| `[BASIC]` | `ENABLED` (**0**), `AUTOSELECT_FITTING_TYRES` (1), `ALTER_SPECIAL_SURFACES` (1) | custom offroad physics (gravel, ice); street tyres chosen on rally tracks; apply ice physics to older mod tracks with ice (list: 3.5) | Y: "requires custom physics to be active"; not available online |
| `[FFB]` | `SAVER` (1), `SAVER_SENSITIVITY` (1), `GRAVEL_FFB_STRENGTH` (1), `FFB_BASE_MULT` (1.3), `FFB_LOW_GAIN` (8), `FFB_HIGH_REDUCE` (8) | FFB limiting and strength on gravel | Y |
| `[DEV]` | `EXTRA_TYRES_ALWAYS_PRESENT` (0), `EXTRA_FFB_MULT` (0.85) (hidden) | developer | Y |

#### 3.1.7 `new_behaviour.ini` (33 AI / physics-adjacent keys of 78 active lines; "all of these won't affect online")

| Section | Keys (default) | Meaning |
|---|---|---|
| `[AI_TWEAKS]` (17 with the hidden `FIX_SHORT_AXLE_TRACK`) | `NEW_LATERAL_GRIP_ESTIMATION` (1; 0 off, 1 new, 2 new + braking for all cars), `FIX_GRIP_MULT` (1), `FIX_SHORT_AXLE` (1; `FIX_SHORT_AXLE_TRACK` 0.25 hidden), `FIX_LOW_LEVEL` (1), `START_REDUCE_REDLINING` (1), `START_STRAIGHTEN_TRAJECTORY` (1), `ACT_IMPATIENT` (1), `TRAJECTORIES_FOR_DUMMIES` (1), `CAUTIOUS_STEERING` (1), `REMOVE_STEER_LOOKAHEAD_SPEED_LIMIT` (1), `FIX_UPSHIFT_THRESHOLD` (1), `FIX_DOWNSHIFT_THRESHOLD` (0), `ADJUST_SHIFTING_FOR_SLOW` (1), `STOP_TELEPORTING_STUCK` (0), `NEW_AI_SMOOTHING` (1, visual / audio only), `REDUCE_STABILIZATION` (0) | what AIs do with a car; hooks on `AIDriver::*` (2.3) |
| `[RAIN]` (2) | `AVOID_SIDES` (1), `AVOID_PUDDLES` (0) | AI lane choice in rain |
| `[AI_RACE_COOLDOWN]` (4) | `ENABLED` (0), `TIME_LIMIT_MIN` (30), `LIMITER` (0.2), `SPEED_LIMIT` (120) | AIs slow down after a race ends |
| `[AI_RACE_RETIREMENT]` (4) | `REJOIN`, `REJOIN_MESSAGE` (0), `PIT_WITH_DAMAGE`, `TELEPORT_WITH_DAMAGE` (1) | what damaged AIs do |
| `[AI_RACE_RUBBERBANDING]` (5) | `ENABLED` (0), `CHANGE_TIME` (60), `CHANGE_DISTANCE` (200), `DECREASE_AHEAD` (0.03), `INCREASE_BEHIND` (0.1) | AI level offset by gap to the player |
| `[AI_RACE_ADAPTATION]` (1) | `ENABLED` (0) | experimental adaptive AIs |

Not counted: `[AI_SPLINES]` (7, spline data handling), `[WRONG_WAY]` (2), `[RESET_CAR]` (2), `[AI_FLOOD]` (17), `[CUSTOM_AI]` (4), one per-track section (4).

#### 3.1.8 small physics-related sets in other files

| File / section | Keys (default) | Meaning | Count |
|---|---|---|---|
| `graphics_adjustments.ini [ORIGIN_SHIFT]` | `ENABLED` (1) (`DEBUG_MODE` commented) | shifts the world origin for rendering; comment: "to reduce physics shaking activate double precision physics engine in general settings" | 1 |
| `rain_fx.ini [BASIC]` | `ENABLED` (1), `AUTOSELECT_RAIN_TYRES` (1) | RainFX (wet grip, puddles); virtual rain compound; auto-pick rain tyres at start | 2 |
| `rain_fx.ini [RACING_LINE_DEV]` (hidden) | `ENABLED` (1), `FADING_PERIOD` (600 s), `SPEED_THRESHOLD` (10, 90 km/h), `BODY_RADIUS` (0.6, 1.2), `BODY_MULT` (0.01), `WHEELS_MULT` (0.01), `SPLASH_MULT` (3), `SPREAD_MULT` (20) | cars dry the line; synced online with `physics.export/importRacingLineAsync` | 8 |
| `weather_fx.ini [TWEAKS]` | `INCREASE_CONDITIONS_LIMITS` (1) | allow wind above 35 km/h and temperature outside 0..36 C | 1 |
| `gamepad_fx.ini [JOYPAD_ASSIST]` | `ENABLED` (**0**), `IMPLEMENTATION` (gamepad_force) | Lua script replaces pad steering / pedals (`extension/lua/joypad-assist`: 8 variants incl. gyro) | 2 |
| `small_tweaks.ini [RULES]` | `DISABLE_TELEPORT_TO_PITS` (0), `DISABLE_SESSION_RESTART` (0) | offline rules | 2 |
| `small_tweaks.ini [FFB_ADJUSTMENTS]` | `STEP` (0.05), `SHOW_MESSAGE` (1) | FFB hotkeys step | 2 |
| `data_manifest.ini [ASSISTS]` | `_EXT_RAIN_PHYSICS` (1) | launcher "Rain physics" assist (the dll string at RVA 0x2242d18 is read by function 0x7f8ae0) | 1 |
| `data_manifest.ini [INPUT_GROUP_2]` "Car (extended physics)" | `__EXT_ENGINEMAP_UP` (Ctrl+E), `__EXT_TC2_UP` (Ctrl+Y), `__EXT_TC2_DOWN`, `__EXT_DRIVER_SHIFT_LEFT`, `__EXT_DRIVER_SHIFT_RIGHT`, `__EXT_SPEED_LIMITER`, `__EXT_MANUAL_CUTOFF` | bindings that only mean something to cars with scripts / extended data | 7 |

Totals: general 76 + ffb 24 + surfaces_fx 11 + new_behaviour 33 + graphics 1 + rain 10 + weather 1 + gamepad 2 +
small 4 + data_manifest 8 = **170**.

Files with **zero** physics keys: `tyres_fx.ini` (visual only), `track_adjustments.ini` (`ENABLED` "doesn't do
anything"), `skidmarks_fx.ini`, `particles_fx.ini`, `lighting_fx.ini`, `smart_shadows.ini`, `grass_fx.ini`, ...

#### 3.1.9 Keys the DLL reads that no shipped ini documents (string pool, [dll])

| Key | RVA | Where it appears | Meaning (when known) |
|---|---|---|---|
| `FORCE_BASE_PHYSICS` | 0x22aa298 | settings loader 0x1387350 | forces the always-on subset only; a session / server configuration key, logged as "not available with given session settings" (2.4f) |
| `INTEL_EMBREE_ROBUST` | 0x22a9960 | same | robust ray-triangle tests (ini of 0.2.11: "use it if car falls through the ground on triangle edges") |
| `LIMIT_CONTACTS` | 0x22a99e8 | same | cap on contacts [interp] |
| `SIMPLIFY_COLLISIONS` | 0x22a9ae0 | same, among tool-run flags (`HEADLESS_MODE`, `SINGLE_THREAD`, `NO_DIRECT_INPUT`) | for headless tool runs [interp] |
| `ICE_EVERYWHERE` | 0x2269fe0 | new-mode manifest loader | ice physics on every surface (frozen-drive, p2p-1v1, splitscreen use it) |
| `ALLOW_PHYSICS_ALTERATIONS`, `FORCE_EXTENDED_PHYSICS` | 0x226a048, 0x226a080 | new-mode manifest | see 5.1 |
| `ALLOW_CPHYS_SCRIPT`, `CPHYS_PRO` | 0x2242b28, 0x2242c90 | car loader 0x7f7b00 area | gate for a car's physics script and a "pro" variant [interp] |
| `DISALLOW_SURFACESFX` | 0x22031a0 | near the SurfacesFX module | per-track / server veto of Surfaces FX [interp] |
| `DISABLE_RAIN_PHYSICS`, `EXTRA_RULES`, `EXTRA_TWEAKS` | 0x2214b70, 0x2214c78, 0x21fc4d0 | session-information function 0x97fb20 | server-side rule names [interp] |
| `PIT_ALTITUDE` | 0x228bac8 | function 0x1126df0 | source of `baseAltitude` ("Required to sync altitude AC_PIT_0 is missing!"); where the value 418 comes from is not in any text file |

### 3.2 Data-file extensions CSP adds to cars and tracks

Sources: shipped Lua SDK comments and examples, the installed mod cars (via `prior/alpha2026_csp.md` and `content_scan.json`,
no car file was opened), the DLL string pool (file attribution by neighbouring strings is `[interp]` unless an
installed car shows it). **None of these names exist in plain `acs.exe`** (prior section 2 method).

#### 3.2.1 Which cars carry extended data on this PC [scan]

| Car | Marker | Suspension | Script |
|---|---|---|---|
| `vrc_formula_alpha_2026_csp` | `VERSION=extended-2` [prior], `[_EXTENSION] REQUIRED_VERSION=4077`, `RAIN_DEV=0` | COSMIC both axles, `USE_DWB2=0`, `DAMPER_LUTS=1`, `TORQUE_MODE_EX=2` | `data/script.lua` 345,676 bytes, 47 encrypted modules; `script_render.lua` 187,419 bytes |
| `vrc_formula_alpha_2025_csp` | `REQUIRED_VERSION=3465`, `RAIN_DEV=1` | COSMIC, `USE_DWB2=1`, `DAMPER_LUTS=1` | 37 `script_*.lua` data files |
| `vrc_formula_beta_2024_csp` | `REQUIRED_VERSION=3281`, `RAIN_DEV=1`, `SCRIPT_INPUTS_REPLAY=100:uint8, 101:half, ...` | COSMIC, `USE_DWB2=1`, `DAMPER_LUTS=1`; `FLEX_HINGE`, `SUB_BODY_R` bodies | 32 `script_*.lua` |
| `vrc_formula_lithium_2023_csp` | `REQUIRED_VERSION=1937`, `RAIN_DEV=1` | **DWB** with `USE_DWB2=1`, `TORQUE_MODE_EX=2`, `DAMPER_LUTS=0`, `[FRONT]/[REAR] MOTION_RATIO=<lut>` | `script.lua` |
| `urd_darche_992_23` | `[_EXTENSION] REQUIRED_VERSION=3040` | unreadable | `script.lua` + 4 `a_urd_*.lua`; **146 data files are scrambled text** (protected by the mod maker; not touched) |
| `vrc_formula_alpha_2025` / `_2026` / `_beta_2024` / `_lithium_2023` (plain twins) | no `_EXTENSION` | DWB | none (`ext_car_controls.ini` on the 2025 one) |

`REQUIRED_VERSION` is a CSP **build number** (the car's shared config tests `read('data/car.ini/_EXTENSION/REQUIRED_VERSION', 0) == 0` to tell versions apart [prior]); the installed build is 4184.

#### 3.2.2 `car.ini`

| Section / key | Value seen | Meaning | Evidence |
|---|---|---|---|
| `[HEADER] VERSION` | `extended-N`, `extendedV2-N` | switch, N = original value (2.2) | [dll] |
| `[_EXTENSION] REQUIRED_VERSION` | 1937..4077 | lowest CSP build | [scan] |
| `[_EXTENSION] RAIN_DEV` | 0 / 1 | developer flag for rain tyres [interp] | [scan] |
| `[_EXTENSION] SCRIPT_INPUTS_REPLAY` | `100:uint8, 101:half, ...` | which `controllerInputs` indices are recorded in replays and with which type | [scan] |
| `[CONTROLS] RACK_TRAVEL_LUT` | `setup_steer_rack.lut` | steering wheel angle to rack travel table | [prior] |
| `[CONTROLS] LINEAR_STEER_ROD_RATIO` | stock key, but read by CSP code too | | [dll 0x2244570] |
| `[CONTROLS]` `POWER_STEERING_SETTING`, `STEER_STRENGTH`, `POWER_STEERING_INPUT_MULT`, `POWER_STEERING_MAX_TORQUE` | - | power-steering map (log "POWER STEERING MAP LOADED") | [dll 0x22445a8..0x2244600] |
| `FFB_LOW_PASS_HZ` | - | steering FFB low-pass | [dll 0x22446e0] |
| `CG_REF`, `CG_SHIFT`, `CG_OFFSET_DEFAULT`, `BALLAST_MASS` | - | centre-of-gravity shift and movable ballast (VRC `setup_cg_shift.lut`) | [dll 0x22438dc..0x2243910], [interp] file |
| `TOTALMASS`, `EXPLICIT_INERTIA`, `REAL_MASS_ONLINE`, `BODY_` ... | - | read near `[BASIC]`; online uses the car's real mass | [dll 0x221f068..0x221f1d0] |

#### 3.2.3 `suspensions.ini` (COSMIC, DWB2, links, flex)

| Section / key | Meaning | Evidence |
|---|---|---|
| `[FRONT]`/`[REAR] TYPE=COSMIC` | CSP's body-and-joint suspension (log "Creating cosmic suspension for wheel N"; RTTI `cphys::cphys_suspension_cosmic`, `cphys_suspension_dummy`) | [prior] [dll 0x2244640, 0x25f9360] |
| `[_EXTENSION] USE_DWB2`, `DAMPER_LUTS`, `TORQUE_MODE_EX` (=2 everywhere), `FIX_PROGRESSIVE_RATE`, `SEPARATE_BSH_GAPS` | DWB2 = CSP's own double wishbone ("Using AC Patch Custom DWB Suspension"); `DAMPER_LUTS` = use `dampers.ini` | [scan] [dll 0x2245390] |
| `BODY_n_NAME` (`_MASS`, `_INERTIA`, `_POS`, `_CARPOS`, `_MIRROR`, `IS_CHASSIS_BODY`, `USE_FOR_ARB`, `MIN_VELOCITY`, `MAX_DAMAGE`, `HUB_MASS`, `HUB_INERTIA`, `ANGULAR_INERTIA`) | extra rigid bodies (rocker, U-bar, ballast, `SUB_BODY_R`) | [prior] [dll 0x22447c0..0x2244888] |
| `HJn`, `Jn`, `DJn`, `STEER_JOINT_n`, `SLIDER_n`, `J0=U_BAR_BJ`, `DJ6=ARB_TRI`, `BALL_JOINT_*`, `HINGE_JOINT_*`, `DISTANCE_JOINT_*`, `WISHBONE_*`, `_POS_A`/`_POS_B`/`_POS_A_SETTING_`/`_POS_B_SETTING_`, `_LENGTH_OFFSET_`, `PARITY`, `CARPOS0/1`, `ELECT_LOG` | joints between bodies; `_SETTING_` links a point to a setup item | [prior] [dll 0x22449a0..0x2245098] |
| `[FRONT/REAR_TORSION_n]` (`RATE`, `PRELOAD_TORQUE`, `HINGE`, `PARITY`), `[..._COILOVER_n]` (`INSTALL_RATE`, `RATE`, `BODY_A/B`, `POS_A/B`, `MIN/MAX_LENGTH`, `END_RATE`, `END_VTAPER`, `END_VMAX`, `BUMPSTOP_GAP`, `PULL_FORCE`, `PRELOAD(_FORCE)`, `FLIP_PRELOAD`, `HELPER_RATE`, `HELPER_TRAVEL`, `COMPENSATE_MASS`, `MASS_INNER/OUTER`, `INSTALL_DAMP`), `[..._DAMPER_n]`, `[..._SPRING_n]` (`SPRING_LUT`, `CALIBRATION_LUT`), `MOTION_RATIO`, `DAMPER_MOTION_RATIO` | torsion bars, coil-overs, dampers, springs with luts | [prior] [dll 0x2244bb8..0x2245268] |
| `[ARB] EXTEND`, `FRONT_MOTION_RATIO`, `REAR_MOTION_RATIO` | anti-roll bar mechanism | [prior] |
| `ENGINE_TORQUE_BODY`, `ENGINE_TORQUE_HUB_POS`, `ENGINE_TORQUE_CHASSIS_POS`, `ENGINE_TORQUE_*_CARPOS` | where engine reaction torque is applied | [scan] [dll 0x22447f0..0x2244978] |
| `OVERRIDE_ERPCFM`, `TOEVECTOR`, `STEERVECTOR`, `ADD_TO_FFB`, `RACK_` | joint softness, steering geometry | [dll] |
| `[_AC_LINK_ALTERATION] PRINT_OUT=1` | prints every suspension joint to the CSP log (index for `ac.suspensionJoint*`) | [sdk] |
| `[_EXTRA_LINK_n]` `FROM_POS`, `TO_POS`, `ANCHOR`, `RIGIDITY`, `FIXED`, `DISTANCE`, `LINKS`, `DEACTIVATE`, `WHEELS` (`LEFT_FRONT`, `RIGHT_FRONT`, `LEFT_REAR`, `RIGHT_REAR`), `DEBUG_COLOR` | extra rigid links between car points | [dll 0x2243f1c..0x2244018] |
| `FRONT_GEOMETRY`, `REAR_GEOMETRY` | reference geometry of the "Virtual K&C" tests (`knc.ini`, files under `/cphys/K&C/`: heave, roll, lateral / longitudinal opposed and parallel, MZ) | [dll 0x2245520..0x2245b78] |
| `[_EXTENSION_FLEX]` `TORSIONAL_STIFFNESS`, `TORSIONAL_DAMPING`, `VERTICAL_BENDING`, `LATERAL_BENDING`, `LATERAL_SHEAR`, `VERTICAL_SHEAR`, `LONGITUDINAL`, each `_STIFFNESS` / `_DAMPING`, `ERP_P`, `ERP_R`, `CFM_P`, `CFM_R`, `USE_FORCES`, `COMPACT_ARRANGEMENT`, `DEBUG` | **chassis flex**: car body modelled as flexible joints (`FLEX_HINGE` in beta 2024) | [dll 0x2242aa0, 0x2243940..0x2243a00], function 0x811810 |
| `USE_BS_INI`, `BUMPSTOP_LUT`, `BUMPSTOP_GAP`, `BUMPSTOP_NUM_`, `BUMP_STOP_LUT` | selects the bump-stop model ("Using new / old bumpstop model") | [dll 0x2245298, 0x2245370] |
| `ac.PhysicsDebugLines.CosmicBodies` | outlines of cosmic bodies in CSP Debug | [sdk] |

#### 3.2.3a Stock suspension keys CSP's DWB2 / links still need
The three installed COSMIC cars keep `TYPE`, `RIM_OFFSET`, `BASEY`, `TRACK`, `HUB_MASS`, `TOE_OUT`, `STATIC_CAMBER`, `[BASIC] WHEELBASE`, `CG_LOCATION`, `[ARB] FRONT/REAR`; everything a DWB needs (`WBCAR_*`, `WBTYRE_*`, spring, damper, bump-stop, packer keys) is absent [prior].

#### 3.2.4 `dampers.ini`, `bumpstops.ini`

| File | Keys | Meaning |
|---|---|---|
| `dampers.ini` | `[HEADER] ENABLE`; `[FRONT_C0/C1]`, `[REAR_C0/C1]`: `BUMP_SLOW_<n>`, `BUMP_FAST_<n>`, `REBOUND_SLOW_<n>`, `REBOUND_FAST_<n>`, `FORCE_REF`, `FRONT_BLOWOFF`, `REAR_BLOWOFF`, `DAMPER_BLOWOFF_`; log "Loading damper hysteresis params" | one force-velocity table per click setting (64 `damper_*.lut` files in the 2026 car); `C0` corner unit, `C1` heave unit; blow-off valves |
| `bumpstops.ini` | `[HEADER] ENABLE`; `[FRONT_C0]`, `[REAR_C0]` (`DEFAULT`, `STACK`); `[..._n]` `LUT`; `FRONT_HEAVE`, `REAR_HEAVE`; `bumpstops.ini` string at RVA 0x2243aa0 | rubber stack force-travel tables selectable in setup |

#### 3.2.5 `tyres.ini`

| Section / key | Meaning | Evidence |
|---|---|---|
| `[THERMAL_MODEL] VERSION=4` (`TMv%d` in the debug panel; "Legacy fallback below v4") | selects CSP's layered thermal model | [prior] [dll 0x2233d84] |
| `[_EXTENSION]` `EXTENDED_RAYTRACING`, `LATERAL_RAYS`, `LONGITUDINAL_RAYS`, `MAX_RAY_ANGLE`, `MAX_RAY_WIDTH_RATIO`, `DISABLE_RAY_DOUBLING`, `PRACTICAL_TEMP_RATIO`, `USE_NEW_RR_SLIP`, `ENABLE_RAY_FORCE_WEIGHTING`, `USE_NEW_VKM`, `SLIP_VERSION`, `RIM_SPRING_RATE`, `RIM_DAMPING_RATE`, `USE_TREAD_COVER` (14) | multi-ray contact patch, rim contact, tread cover; debug text "Extended raytracing, hits: N" | [prior]; `RIM_*` also [dll 0x2242dd8] |
| per compound (26 new keys): `OVERRIDE_BLANKETS_TEMP`, `RADIUS_ANGULAR_K_2`, `DX_CAMBER_REF`, `DX_CAMBER_MULT`, `ROLLING_RADIUS_MULT`, `CAMBER_SPRING_MULT`, `FALLOFF_LEVEL_CURVE`, `FALLOFF_SPEED_CURVE`, `FALLOFF_YSPEED_MULT`, `PRESSURE_GROWTH_GAIN`, `SIDEWALL_LOSS_FACTOR_Z`, `SIDEWALL_K_MULT(_X)`, `WEAR_RATE`, `SAT_PEAK_K`, `SAT_MULT`, `SAT_MIN_REF`, `SAT_LOAD_K`, `SAT_MULT_K`, `SAT_SPEED`, `COMBINED_FACTOR_1`, `COMBINED_FACTOR_BRAKE_MULT`, `COMBINED_FACTOR_LOAD_K`, `DROPOFF_FACTOR_0/1`, `TYPE_HINT`; INT / WET also `TREAD_COVER`, `TREAD_DEPTH`, `TREAD_CLEAR_K` | [prior]; `TYPE_HINT`, `TREAD_*`, `OVERRIDE_BLANKETS_TEMP` also [dll 0x2245bb8, 0x2246498, 0x2246988] |
| per compound `[THERMAL_*]` (17 new): `PERFORMANCE_MAP` (2D lut), `BULK_THICKNESS`, `SIDEWALL_THICKNESS`, `VOLUMETRIC_HEAT_CAPACITY`, `CONDUCTIVITY`, `EFFUSIVITY`, `WEAR_K`, `SURFACE_TO_AMBIENT(_SIDEWALL)`, `ENGINE_BAY_MIX`, `RADIATION_CONSTANT`, `CARCASS_ROLLING_K`, `FLEX_SPEED_COEFF`, `INTERNAL_CONVECTION_K`, `RIM_TO_AMBIENT`, `BRAKE_TO_RIM`; `PERFORMANCE_CURVE`, `COOL_FACTOR` are *removed* (stock keys the plain loader then asks for in vain) | [prior]; the DLL has a 40-row thermal parameter table ("Bulk Roll Resist", "Core to SW", "SW Rim Conduction", ... RVA 0x2245d70..0x2246198) | 
| `TYPE_HINT` values | `SLICK`, `SEMISLICK`, `TREADED`, `RACING_TREADED`, `VINTAGE_PERFORMANCE`; `TYPE_HINT_MODIFIER`; auto-detected otherwise ("type determined as SLICK") | [dll 0x2246671], [log-csp] |
| generated tyres | `GENERATE_EXTRA_TYRES`; virtual **rain** compound from the first slick ("Creating rain tyres from: `Slick Soft`"); ice compounds `ICE-R` (Ice Rally), `ICE-S` (Ice Street); `data_apps_virtual_tyres_fix.txt` lists the virtual-tyre names | [dll 0x2246bb8..0x2246c70] [prior] |
| `THERMAL2_FRONT/REAR(_n)` | second thermal section family [interp: a second model generation] | [dll 0x2246a40..0x2246ca8] |
| Falloff | CSP re-reads `FALLOFF_LEVEL` / `FALLOFF_SPEED` ("Tire constant falloff found: 0.8 / 3") and the camber curve | [log-csp] |
| SFX keys `_SFX_LOAD_MULT_K`, `_SFX_BASE_FORCE_MULT_K`, `_SFX_SIDE_LIMIT_K`, `_SFX_SIDE_MULT_K`, `_SFX_TREAD_LIMIT_K`, `_SFX_TREAD_MULT_K`, `_SFX_SLIP_LUT`, `_SFX_THREAD_LUT`, `_SFX_LOAD_LUT`, `_SFX_ICE_BASE_STIFF`, `_SFX_ICE_BASE_MULT_K`, `_SFX_ICE_DAMP_MULT_K`, `_SFX_ICE_FORCE_MULT_K`, `_SFX_ICE_BASE_SIZE` (FRONT_/REAR_ prefix) | internal parameters of the Surfaces FX tyre model; message "Obsolete tyre model" for cars the model cannot handle | [dll 0x2202dd8..0x2202fb0] |
| `[TRACTION_CONTROL_2]` (in `electronics.ini`): `SETTING_1`, `SETTING_2`, `TC2_SETTING`, `LAT_G`, `LON_G`, `SPEED_KPH`, `SLIP_ACCEL`, `ENGINE_ACCEL`, `SLIP_DIFF`, `SLIP_DIFF_RATIO`, `TARGET_SLIP`, controller `INPUT`, `COMBINATOR`, `UP_LIMIT`, `DOWN_LIMIT` | CSP's second traction control: slip target, gain / cut 1D and 2D lookups, P / I / D controllers (debug lines "TC Slip Target", "TC Slip Error", "Traction Control Cut"); bound to `__EXT_TC2_UP/DOWN` | [dll 0x2246300..0x22463d8] |
| SFP | `ext_config.ini` key `SFP_TYRE_MODEL`, file `data/sfp.ini`, Lua modules `alt_sfp1(.luac)`; message "SFP tyre model is not compatible with car physics scripts" | alternative tyre model; **exclusive with cphys scripts** | [dll 0x21ef6c0, 0x2242ad0, 0x2242b40] |

#### 3.2.6 `engine.ini`, `drivetrain.ini`, `brakes.ini`, `ers.ini`, `aero.ini`, `colliders.ini`, `setup.ini`

| File | New keys | Meaning | Evidence |
|---|---|---|---|
| `engine.ini` | `[HEADER] POWER_CURVE_STANDARD`, `HUMIDITY_REF`, `TURBO_VERSION`; `[ENGINE_DATA] AMBIENT_EFFECT`, `MECHANICAL_EFFICIENCY`, `IDLE_THROTTLE`, `IDLE_CUTOFF`; `[FUEL_CONSUMPTION] MAX_FUEL_FLOW_LUT`, `MAX_FUEL_FLOW`, `LOG_FUEL_FLOW`; `[TURBO_n] EXT_GAS_CURVE`, `EXT_SPIN_DELAY`, `FLOW_ON_CUT`; `ENGINE_MAPS`, `PEDAL_MAPS`; `WATER_COOL_SPEED_K`, `WATER_THERMAL_MASS`; `BODY_TORQUE_MULT` | humidity / ambient effect on power, fuel flow log, turbo external gas curve, engine / pedal maps (`__EXT_ENGINEMAP_UP`, `__EXT_PEDALMAP_UP`), water thermal model. Only `EXT_GAS_CURVE`, `EXT_SPIN_DELAY`, `WATER_*`, `ENGINE_MAPS`, `PEDAL_MAPS`, `BODY_TORQUE_MULT` exist as strings in the DLL: the others (`HUMIDITY_REF`, `AMBIENT_EFFECT`, ...) are **not** in the DLL string pool, so they are read by the car's own script or are hashed [interp] | [prior] [dll 0x221f120, 0x2242ed0, 0x2243d78..0x2243e40] |
| `drivetrain.ini` | `[DAMAGE]` `TORQUE_THRESHOLD`, `TORQUE_DAMAGE_K`, `ENGINE_TORQUE_THRESHOLD`, `ENGINE_TORQUE_DAMAGE_K`, `CLUTCH_TORQUE_THRESHOLD`, `CLUTCH_TORQUE_DAMAGE_K` (seen in the 2023 car); `DIFF_FRICTION_FACES`, `DIFF_RAMP_COAST`, `DIFF_RAMP_POWER`, `DIFF_PRELOAD_EXT`; `[_EXTENSION_AUTOCLUTCH]` `ENGAGING_RPM_THRESHOLD`, `ENGAGING_GAS_THRESHOLD`, `SLIP_CLUTCH`, `SLIP_RPM_THRESHOLD`; `CLUTCH_LUT`; `AWD2_INVERTED`; `SUPPORTS_MANUAL_CUTOFF`; `[SCRIPT_AUTOMATIC_GEARBOX]` `SHIFT_TIME`, `LOCK_RATIO_GAP`, `BRAKE_DAMAGE_THRESHOLD_KMH` (read by the shipped Lua library, 3.4) | | [scan] [dll 0x2243ad8..0x2243b48, 0x2242d48..0x2242e90] [lua] |
| `brakes.ini` | `[_EXTENSION] ENABLE`, `USE_ADVANCED_SYSTEM`; `[TEMPS_FRONT/REAR]` `COOL_SPEED_FACTOR_0`, `CORE_TRANSFER_IN/OUT/AIR`, `DEFAULT_BLANKING`; `BRAKE_*`; function 0x812bb0 (26,500 bytes) builds "front / rear brake hysteresis controllers" | disc core temperature, duct blanking, hysteresis | [prior] [dll 0x2243a14..0x2243a80] |
| `ers.ini` | stock keys only; CSP's native ERS reads `[FRONT_MOTORS]`, `[KINETIC]` | | [prior] [dll 0x2202cd0] |
| `aero.ini` | `[MAP_n]` (`NAME`, `MAP_CL_RH`, `MAP_CD_RH`, `MAP_BALANCE_RH`, `MAP_FORCE_RH`, `MAP_YAW_*`, `MAP_ROLL_*`, `MAP_STEER_*`, `MAP_SPEED_*`, `FRONT_RH_OFFSET`, `REAR_RH_OFFSET`, `CL_MULT`, `CD_MULT`, `BALANCE_OFFSET`, `INTERPOLATION` (`LINEAR`, `CUBIC`), `DRAG_OFFSET`, `FRONT_FORCE_DELAY`, `REAR_FORCE_DELAY`, `DRAG_FORCE_DELAY`); `AEROMAP` setup section with `EXT_CONTROLLER=ctrl_aero_maps.ini` (3 maps in beta 2024); aero "fans" (debug "Aero fan N") | 2D ride-height maps with corrections and force delays; `setWingGain` | [prior] [scan] [dll 0x22434b0..0x22437f8, 0x2242c40] |
| `colliders.ini` | `[_EXTENSION] VERSION`, `EXTEND`; per box `STIFFNESS`, `DAMPING`, `FRICTION`, `MAX_WEAR`, `WEAR_COEFF`, `ALLOW_LUA_RESET`, `SPARKS_AS` (also `COLLIDER_n_SPARKS_AS` in the CSP car config); new collider kinds `COLLIDER_CAPSULE`, `COLLIDER_CYLINDER` with `COLLISION_PARAMS`, `LAYER`, `RIGID_DIRT_COLLISIONS`, `RIGID_WALLS_COLLISIONS`, `RIGID_FLOOR_COLLISIONS`; curve set `STIFFNESS`, `SOFT_CFM(_CURVE)`, `SOFT_ERP(_CURVE)`, `DAMPING(_CURVE)`, `FORCE_CURVE`, `FRICTION_CURVE`, `BOUNCE(_CURVE)`, `INTENSITY(_CURVE)`, `MAX_DEPTH`, `RIGID_WITH_BOXES`, `RIGID_WITH_BODIES` (log "collider constant friction read, mu: -1") | plank wear, compliant contact curves | [prior] [scan] [dll 0x2247f28..0x2248158, function 0x9472f0, 0x683eb0] |
| `setup.ini` | `[CUSTOM_SCRIPT_ITEM_n]` (`ID`, `DEFAULT`, ...), `EXT_CONTROLLER`, `EXT_ENABLED_WITH`, `EXT_VISIBLE_WITH`, `COCKPIT_SWITCH`, `SPEED_LIMITER`, `DEFAULT_ENABLED`, `DISPLAY_MULTIPLIER`, `Preset*`, `PARSER=INIPP` | script-owned setup items (2,271 in the 2026 car) | [prior] [sdk] [dll 0x2244348..0x2244560] |
| `ext_car_controls.ini` | per control: `TAB`, `NAME`, `DN`, `UP`, `DN_LABEL`, `UP_LABEL`, `POS`, `POS_LABEL`, `ACTIVATION`, `HOLD_MODE`, `EXT_PHYSICS`, `LUA`, `HELP` | **VRC's "Extended Controls" app convention, not CSP**: file lives in the car's `extension/`; sections are `controls.ini` names (`__EXT_CAR_*`) | [lua] `apps/lua/ext_controls` |
| `script.lua`, `script_render.lua`, `script_params.ini` (+ `script_*.lua`) | the physics script (3.4), a render-thread module, the mod's own parameters | `script_params.ini` is read by the script, not by CSP | [prior] [sdk] |
| `ctrl_*.ini` `INPUT=` | stock controller inputs (24) + **35 names CSP adds** + `SCRIPT_0..255` | see below | [dll 0x2242f10..0x2243448] |

**Controller inputs CSP adds** (UTF-16 strings, `DynamicController::getInput` hook RVA 0x5c1eb0) [dll]: `CLUTCH`,
`HEADLIGHTS`, `HANDBRAKE`, `GEAR_ALT`, `DAMAGE_ENGINE`, `DAMAGE_GEARBOX`, `LIGHT_EXTRA_A` .. `LIGHT_EXTRA_T` (20),
nine axle-difference signals `AXLES_DIFFERENCE_{ABSOLUTE,RELATIVE,SIGNED}`, `FRONT_AXLE_DIFFERENCE_{...}`,
`REAR_AXLE_DIFFERENCE_{...}` (35 names) and `SCRIPT_0..SCRIPT_255` (written from the script through
`controllerInputs`; for emissives `CPHYS_SCRIPT_n`) [sdk]. Plain `acs.exe` prints "INPUT UNDEFINED" for all of them and reads 0.

#### 3.2.7 Track files

| File / section / key | Meaning | Evidence |
|---|---|---|
| `data/surfaces.ini [SURFACE_n] WAV_PITCH=extended-N` | extended track physics on (2.2) | [sdk] [dll] |
| `[_SCRIPTING_PHYSICS]` 8 keys | permissions (3.5) | [sdk] [dll] |
| `surfaces.ini [_EXTENSION]` (function 0x94dcd0, 7,233 bytes) | `GRAVITY`, `RANDOMIZED_CARS_POSITIONING`, `ALIGNED_CARS_POSITIONING`, `DISABLE_SMOOTH_COLLISIONS`, `FORCE_DOUBLESIDED_RAYCASTS`, `ONLINE_ONLY`, `_VERIFY_INTEGRITY` (`CHECKSUM`, `REQUIRED_VERSION`, `_FROM`: "Required file ... is modified or damaged") | [dll 0x2248250..0x2248398] [interp] file |
| `CUSTOM_RAYCASTER` + `MODULE` | a track may ship a native module with exports `csp_Initialize_v1`, `csp_Raycast_v1`, `csp_GetLastError_v1`; loaded by `LoadLibraryW` / `GetProcAddress` in function 0x15c9db0 with an integrity check; "Module did not register any surfaces"; needs double precision + custom raycasting enabled | [dll 0x22bf9c0..0x22bfb60, 0x22484a0] |
| `data/*` track objects | `physics.RigidBody`, `ac.onTrackPhysicsObjectCarCollision`, `physics.resetTrackPhysicsObject`: dynamic track objects (`AC_POBJECT_*` nodes) tweaked by `TWEAK_TRACK_OBJECTS` | [sdk] [dll 0x2220088] |
| `extension/config/tracks/**` and `content/tracks/*/extension/ext_config.ini` | `[SCRIPT_...]` track scripts; visual keys. On this PC none sets a physics key | [scan] |

### 3.3 The physics Lua API

Where each API lives (SDK module = script kind; `rules.json` gives the file paths a kind may use):

| SDK module (script kind) | `physics.*` functions | Other physics-relevant API |
|---|---|---|
| `ac_car_cphys` (car **physics** script: `content/cars/*/data*`, no IO) | 13: `getPointVelocity`, `getExtendedSuspensionTravel`, `getExtendedDamperTravel`, 8 `Collider.*` constructors, `allowed`, `raycastTrack` | 95 own `ac.*` functions + 31 car-control functions shared with display scripts + 29-value `ac.CarPhysicsValueID` + 4 state classes |
| `ac_car_scriptable_display` (car **display** / extension script) | the same 13 | the 31 shared car-control functions (they can alter the car too) |
| `ac_track_script`, `ac_tools`, `ac_apps`, `ac_online_script` | **107** unique (113 with 6 declared twice) + class `physics.RigidBody` (27 methods) + `physics.CarControls` | `ac.areCarsColliding`, `ac.areShapesColliding`, `ac.getCollidingShapesContacts`, `ac.onTrackPhysicsObjectCarCollision`, `ac.getCarPhysics`, `ac.setPhysicsDebugLines` |
| `ac_new_modes` | 108 (adds `physics.setUserControlledCar`) | same |
| `ac_background_pw` (physics **worker**, runs at 333 Hz) | 31 unique: the 13 + `getBodyTransform`, `getWheelAngularSpeed`, `addWheelTorque`, `awakeCar`, `addForce`, `setEngineRPM`, `estimateAIControls`, `getTrackObjectStatus`, `getTrackObjectPointVelocity`, `addTrackObjectForce`, `forceUser*For` (4), `offsetUserSteering`, `offsetFinalFFB`, `overrideUserSteering`, `overrideSteering` | no `ac.getCar` at physics rate |
| `ac_background`, `ac_wfx_*`, camera, pp, splashscreen modules | 11 (query + colliders only) or 8 | none |

#### 3.3.1 `physics.*` (track / app / tool / online / new-mode scripts): 107 functions in 17 groups

"I" = documented to invalidate the current lap, "(c)" = only in a stated condition; "NI" = the text says it does not.

| Group | n | Functions |
|---|---|---|
| queries | 6 | `getPointVelocity`, `getExtendedSuspensionTravel`, `getExtendedDamperTravel`, `getCarInputControls`, `allowed`, `raycastTrack` |
| rigid bodies, colliders, track meshes | 10 | `RigidBody`, `addTrackSurface`, `Collider.Box`, `.Sphere`, `.Capsule`, `.Cylinder`, `.Ray`, `.Track`, `.TrackWalls`, `.TrackGround`. `RigidBody` methods (27): `dispose`, `onCollision`, `setTransformation`, `setTransformationFrom`, `setVelocity`, `setAngularVelocity`, `setDamping`, `setMass`, `setSemiDynamic`, `isSemiDynamic`, `setEnabled`, `isEnabled`, `setInWorld`, `isInWorld`, `getSpeedKmh`, `getAngularSpeed`, `getVelocity`, `getAngularVelocity`, `getLastHitIndex`, `getLastHitPos`, `addForce`, `localPosToWorld`, `localDirToWorld`, `worldPosToLocal`, `worldDirToLocal`, `pointVelocity`, `getTransformation` ("requires double precision physics engine") |
| car placement and state | 9 | `teleportCarTo` I, `setCarPosition` I, `setCarVelocity` I, `resetCarState` I, `setCarFuel` I, `awakeCar`, `setGentleStop`, `setCarBodyDamage` I(c: improves), `setCarBallast` I(c: negative) |
| engine, fuel, water | 5 | `setEngineRPM` I(c: unless below 400), `setEngineStallEnabled`, `setCarEngineLife` I(c: improves), `setCarRestrictor` NI, `setWaterTemperature` NI |
| drivetrain, wheels | 5 | `engageGear` I, `lockUserGearboxFor`, `setCarAutoShifter` NI, `addWheelTorque` I, `setWheelAngularVelocity` I |
| hybrid | 2 | `setMGUKDelivery`, `setMGUKRecovery` |
| tyres | 7 | `setGripDecrease`, `setTyresAIMultiplier` ("not recommended"), `blowTyres` NI, `setTyresBlankets` I, `setTyresCompound`, `setTyresTemperature` I, `setTyresVirtualKM` |
| suspension | 1 | `setSuspensionDamage` NI |
| aero, DRS, air | 3 | `allowCarDRS` NI, `setCarDRS` NI, `setAirDensityMultiplier` I |
| forces | 1 | `addForce` I (also `RigidBody:addForce`, `addTrackObjectForce` in workers) |
| collisions, wear | 4 | `disableCarCollisions` I, `setCarCollisions` I (deprecated), `resetColliderWears`, `resetWearPlanks` I (deprecated) |
| world, track objects, rain | 3 | `resetTrackPhysicsObject`, `exportRacingLineAsync`, `importRacingLineAsync` |
| rules, session | 8 | `setAllowedTyresOut`, `setPenalties`, `setWrongWayPenalty`, `setAssists`, `setCarPenalty` I, `markLapAsSpoiled`, `overrideRacingFlag`, `blockTeleportingToPits` (the first four are allowed for new modes even without the rest of the API) |
| controls, steering, FFB | 11 | `lockUserControlsFor`, `setCarNoInput`, `setCarAutopilot` I, `forceUserBrakesFor` I, `forceUserThrottleFor` I, `forceUserClutchFor` I, `forceUserHandbrakeFor` I, `offsetUserSteering`, `offsetFinalFFB`, `overrideUserSteering`, `overrideSteering` |
| identity | 2 | `setDriverName`, `setDriverTeam` (announced online to other clients) |
| workers | 1 | `startPhysicsWorker(script, data, callback)` |
| AI | 29 | `setAILevel`, `setAINoInput`, `setAIDriverName` (online), `setAIDriverTeam` (online), `setAICarPosition`, `setAITimeToStart`, `setAIAggression`, `setAIAeroHint`, `setAITyresHint`, `preventAIFromRetiring`, `setAIShiftingThresholds`, `setAICaution`, `setAILookaheadBase`, `setAILookaheadGasBrake`, `setAIBrakeHint`, `setAIStopCounter`, `setAIThrottleLimit`, `setAITopSpeed`, `setAISteerMultiplier`, `setAISplineOffset`, `setAISplineAbsoluteOffset`, `setAISpline`, `setExtraAIGrip`, `setAIPitStopRequest`, `setAILapsToComplete`, `setAITyres`, `setAIBrakeMistake`, `setAITractionMistake`, `setAIRainTyres` |

Sum 6 + 10 + 9 + 5 + 5 + 2 + 7 + 1 + 3 + 1 + 4 + 3 + 8 + 11 + 2 + 1 + 29 = **107**.

Lap invalidation (SDK text): **25 functions invalidate** (`teleportCarTo`, `setCarPosition`, `setCarVelocity`,
`engageGear`, `resetCarState`, `setCarFuel`, `setCarAutopilot`, `setAirDensityMultiplier`, `resetWearPlanks`,
`setTyresBlankets`, `setTyresTemperature`, `setCarBodyDamage`(c), `setCarEngineLife`(c), `setCarBallast`(c),
`setCarCollisions`, `disableCarCollisions`, `setCarPenalty`, `addForce`, `addWheelTorque`, `setWheelAngularVelocity`,
`setEngineRPM`(c), `forceUserBrakesFor`, `forceUserThrottleFor`, `forceUserClutchFor`, `forceUserHandbrakeFor`);
**7 say they do not** (`blowTyres`, `setSuspensionDamage`, `setWaterTemperature`, `setCarRestrictor`,
`setCarAutoShifter`, `setCarDRS`, `allowCarDRS`). `ac.setTyresBlankets` (car script) also "invalidates the current lap".
Car-script `ac.*` functions are the car's own physics and do not say they invalidate (they cannot reach a lap that is
not the script's own) [interp].

**Online / availability restrictions in the SDK text:** none of the 107 is marked "not online". What the text does say:
`setDriverName`, `setDriverTeam`, `setAIDriverName`, `setAIDriverTeam` "announce the new name online";
`exportRacingLineAsync` / `importRacingLineAsync` are "to sync the state in online races"; `addTrackSurface`
"requires Embree and double precision, not available to scripts without Physics API access"; `RigidBody` "requires
double precision physics engine"; `physics.allowed()` is false unless the track permits (3.5) or the mode sets
`ALLOW_PHYSICS_ALTERATIONS`; and the code (2.4f) turns the experiments off online.

#### 3.3.2 car physics script (`ac_car_cphys`): 95 own `ac.*` functions in 16 groups

| Group | n | Functions |
|---|---|---|
| state access | 3 | `accessCarPhysics`, `accessCarDamper`, `getCarWheelSurface` |
| forces, bodies, mass | 10 | `addForce`, `addHubForce`, `addHubForce2`, `addTorque`, `addCosmicForce`, `addCosmicTorque`, `setExtraMass` (up to 4 anonymous or 8 keyed movable masses, soft link), `awakeCarPhysics`, `addWheelsFeedbackTorque`, `getWheelsInertia` |
| colliders, wear | 3 | `setColliderOffset`, `resetColliderWears`, `getColliderWear` |
| aero, DRS | 2 | `setWingGain`, `allowCarDRS` |
| suspension | 6 | `suspensionJointConfigure` (ERP, CFM), `getSuspensionJointPointTo`, `suspensionJointReseat`, `suspensionJointReseatDistance`, `addSuspensionDamage`, `setAntirollBars` |
| engine, fuel, water | 13 | `overrideEngineTorque`, `setExtraTorque`, `setEngineRPM`, `setEngineRPMIdle`, `setEngineRPMLimit`, `setEngineLifeLeft`, `setEngineStalling`, `setEngineStarterTorque`, `setWaterTemperature`, `setFuelConsumption`, `disableEngineLimiter`, `overrideGasInput`, `computeEngineStep` (basic Kunos engine only) |
| turbo | 7 | `setTurboUserWastegate`, `getTurboUserWastegate`, `setTurboMaxBoost`, `setTurboExtras`, `setTurboExtras2`, `overrideTurboBoost`, `overrideTurboBlowOffValve` |
| drivetrain, gearbox, clutch | 12 | `setGearsFinalRatio`, `setDifferentialCoast`, `setDifferentialPower`, `setDifferentialPreload`, `setAWDCenterDifferentialPreload`, `setGearsGrinding`, `setGearboxTimes`, `setDrivetrainDamageRPMWindow`, `setDrivetrainRootVelocity`, `setDrivetrainDriveVelocity`, `setClutchMaxTorque`, `replaceClutch` (callback gets engine / root velocity, inertias, clutch amount, dt) |
| brakes, ABS | 2 | `overrideBrakesTorque` ("once called the original braking system is disabled for the race"; no effect if the new brake system is on), `overrideABS` |
| tyres | 5 | `setTyreInflation`, `setTyreWearMultiplier`, `shiftPatchTemperatures`, `setTyresBlankets`, `setRainTreadEfficiencyMultiplier` |
| hybrid | 3 | `addElectricTorque`, `setKERSCharge`, `setKERSCurrentKJ` |
| setup, script items | 6 | `getScriptSetupValue`, `setScriptSetupValue`, `getSetupValue`, `onSetupValueChange`, `reportIncompatibleSetup`, `getDynamicController` |
| AI | 7 | `setAIBrakeHint`, `setAIBrakeMistake`, `setAILookaheadBase`, `setAILookaheadGasBrake`, `setAIPitStopRequest`, `setAITractionMistake`, `setExtraAIGrip` |
| steering FFB | 2 | `setSteeringFFB`, `setSteeringDamping` |
| debug drawing | 8 | `drawDebugLine`, `drawDebugArrow`, `drawDebugSquare`, `drawDebugBox`, `drawDebugCross`, `drawDebugText`, `isDebugDrawActive`, `debugSleep` |
| meta | 6 | `extendCarState` (save / load callbacks, replay snapshots), `loadRenderThreadModule`, `overrideCarState`, `overrideSpecificValue`, `showNotification`, `disallowExtraSwitch` |

Total 3 + 10 + 3 + 2 + 6 + 13 + 7 + 12 + 2 + 5 + 3 + 6 + 7 + 2 + 8 + 6 = **95** (the SDK counts 992 functions in
`ac_car_cphys`, 197 "own" including the shared car-control ones).

Shared with display scripts (31): `setTurboWastegate`, `isTurboWastegateAdjustable`, `setABS`, `setTC` (deprecated),
`setTractionControl`, `setAutoShifter`, `setBrakeBias`, `setEngineBrakeSetting`, `setDRS`, `setKERS`, `setMGUHCharging`,
`setMGUKDelivery`, `setMGUKRecovery`, `switchToNeutralGear`, `setHeadlights`, `flashLights`, `setWiperMode`,
`setHighBeams`, `setDaytimeLights`, `setBrakingLightsThreshold`, `setTurningLights`, `setExtraSwitch`,
`isExtraSwitchAvailable`, `setGearLabel`, `overrideTyreSmoke`, `overrideTyreIgnitionPoint`, `pauseScriptUntil`,
`configureSetupItems`, `configureSetupTabs`, `accessExtraSwitchParams`, `ControlButton`.

`ac.CarPhysicsValueID` (29 ids, used with `ac.overrideSpecificValue(id, value, wheel?)`): `ERSRecovery`,
`ERSHeatCharging`, `AWD2MaxTorque`, `FFBMult`, `SteerAssist`, `SteerRatio`, `LinearSteerRodRatio`,
`DrivetrainOpenThreshold` (default 0.1), `DrivetrainClutchOverride`, `DrivetrainDriveVelocityMult`,
`DrivetrainEngagedGear`, `EngineLimiterCycles`, `BrakeTorque`, `HandbrakeTorque`, `BrakeFrontBias`,
`OriginalTCPresent`, `TCEnabled`, `TCThrottleOverride`, `ForcedAutoclutch`, `BlockOriginalTC`,
`AutoShifterUp` / `Down` / `SlipThreshold` / `GasCutoffTime`, `ForcelessTyres`, `TyresVirtualKm`, `TyresDirtLevel`,
`TyresFlatSpot`, `TyresSlipRatio`.

State classes: `ac.StateCphysCar` (52 fields: transform, `gForces`, `localVelocity`, `rpm`, `engineVelocity`,
`drivetrainRootVelocity`, `airDensity` kg/m3, `ambientTemperature`, `engineAirConditionMultiplier`, `gear`,
`targetGear`, controls `gas` / `brake` / `steer` / `clutch` / `handbrake` / `gearUp` / `gearDown` /
`requestedGearIndex` (writable), `wheels[0..3]`, `controllerInputs[0..255]`), `ac.StateCphysWheel` (36 fields: slip,
slip angle, load, depth, `fx`, `fy`, `mz`, temperatures inside / middle / outside / core, `dynamicPressure`,
`surfaceGrip`, `surfaceExtendedType`, `contactPoint`, `contactNormal`, ...), `ac.StateCphysDamper` (6: slow / fast
bump / rebound and the two fast thresholds, read-write), `ac.StateCphysSurface` (13: grip, `sectorID`, `dirt`,
`collisionCategory`, `isValidTrack`, `blackFlagTime`, `sinHeight`, `sinLength`, `isPitlane`, `damping`,
`granularity`, `vibrationGain`, `vibrationLength`). Graphics-rate state for apps: `ac.StateCarPhysics`
(`airDensity` "affected by slip stream", `scriptControllerInputs[256]`, `wings[]`), `ac.StateWheelPhysics`
(`driveTorque` "-1 if extended physics is disabled", `tyreCarcassTemperature` "only with custom physics tyres").

### 3.4 How a cphys Lua script plugs into the step

| Question | Answer | Evidence |
|---|---|---|
| Which file | `data/script.lua` of the car (SDK `rules.json`: `content/cars/*/data*`, `withoutIO: true`); optional `script_render.lua` via `ac.loadRenderThreadModule(name)` (render thread; may add setup tabs; once per car) | [sdk] |
| Callbacks | only two names on the `script` table: `script.update(dt)` "called each physics frame, `dt` is time since the last call, usually around 0.003" and `script.reset()` "when car resets or teleports (pits, new session)". The `script` global is single-instance. | [sdk] `ScriptData` |
| Other entry points | `setTimeout` / `setInterval` timers, event hooks `ac.onCarJumped`, `ac.onLapCompleted`, `ac.onCarCollision`, `ac.onSetupValueChange`, `ac.onReplay`, `ac.onSessionStart`, `ac.onTrackPointCrossed`; `ac.pauseScriptUntil(condition)` skips `update()` until Pitlane / Pits / Reset / Extra switch / MeshInteraction / Collision (only car scripts) | [sdk] |
| Rate | physics rate, i.e. every 3 ms step of the 333 Hz loop; the main-thread `car` and `sim` globals update at **graphics** rate and "might be a few physics frames late", so physics code must use `ac.accessCarPhysics()` | [sdk] |
| Order inside the step | not documented. The hooks around it: `Car::step` (RVA 0x5996f0) copies position / orientation to the avatar then calls the original; `Car::stepComponents` (0x599ec0) and `Car::pollControls` (0x59fdb0 / 0x59fe60) are hooked. Where `update()` is called is not visible in the text [interp: before the stock components so that overrides apply to the same step] | [dll] |
| What it can read | `ac.accessCarPhysics()` (52 + 36 fields), `ac.accessCarDamper(i)`, `ac.getCarWheelSurface(i)`, `physics.raycastTrack`, `physics.getPointVelocity(car, pos, local)`, `ac.INIConfig.carData(car.index, 'file.ini')` (data files, including unpacked values), setup values, `ac.getCar(i)` for other cars (graphics rate) | [sdk] |
| What it can write | the control fields of `ac.StateCphysCar` (`gas`, `brake`, `steer`, `clutch`, `handbrake`, `gearUp`, `gearDown`, `requestedGearIndex`, `isShifterSupported`; "unless controls are locked or the car is black-flagged"), `controllerInputs[0..255]` (read as `SCRIPT_n` by dynamic controllers), the damper fields, and every `ac.*` function of 3.3.2 | [sdk] |
| Data out to the main thread | `controllerInputs` (also `CPHYS_SCRIPT_n` for emissives and `ac.getCarPhysics(i).scriptControllerInputs` for other Lua scripts); `ac.connect(layout)` shared structures (car scripts can connect only to other car scripts; append `car.index` to the key); `ac.ReplayStream` (record per-frame data, **32 bytes per frame for car scripts**, 256 for others); `car.ini [_EXTENSION] SCRIPT_INPUTS_REPLAY` for `controllerInputs` indices; `ac.store` / `ac.load`; `ac.broadcastSharedEvent`; `ac.extendCarState(key, save, load)` | [sdk] [scan] |
| Debug | `ac.drawDebugLine` etc., CSP Debug app tab "Physics" (`ac.PhysicsDebugLines.Script` = 0x4) | [sdk] [lua] |
| Libraries shipped | `shared/physics/automatic-transmission` (7,382 bytes, P/R/N/D with torque-converter-like ratio slip), `automatic-transmission-utils` (9,122), `automated-manual-transmission` (4,536), `rnpd-shifting` (3,814): pure-Lua car-script helpers over `ac.accessCarPhysics`, `ac.overrideSpecificValue`, `ac.setGearsFinalRatio`, `ac.replaceClutch`, `ac.setGearsGrinding`, `ac.setGearLabel`; they read `drivetrain.ini [GEARBOX] SUPPORTS_SHIFTER`, `[GEARS] FINAL`, `COUNT`, `GEAR_n`, `ai.ini [GEARS] UP / DOWN`, and a CSP-defined section **`drivetrain.ini [SCRIPT_AUTOMATIC_GEARBOX]`** (`SHIFT_TIME` 0.2, `LOCK_RATIO_GAP` 0.15, `BRAKE_DAMAGE_THRESHOLD_KMH` 5) | [lua] `internal/lua-shared/physics/` |
| Failure | "Physics is stuck waiting for <car> and its physics script" (watchdog, RVA 0x221fe58..0x221fe90); script errors are logged; an unreadable (encrypted) script just returns | [dll] |
| Encryption | the VRC scripts check `ai.ini [AI_FIX] KEY_A/B/C` and a SHA-256 of the data files (prior 3.13); nothing in CSP's own API is encrypted; **stop there** | [prior] |

### 3.5 Extra surfaces, Surfaces FX, ice and gravel

**What CSP adds to `surfaces.ini`:**
- `WAV_PITCH=extended-N` in a `[SURFACE_n]` section (the SDK says `SURFACE_0`) turns extended track physics on (2.2).
- `[_SCRIPTING_PHYSICS]` with up to 8 keys: `ALLOW_TRACK_SCRIPTS`, `ALLOW_DISPLAY_SCRIPTS`, `ALLOW_NEW_MODE_SCRIPTS`, `ALLOW_TOOLS` [sdk];
  `ALLOW_APPS`, `ALLOW_CUSTOM_AI_MANIPULATION`, `ALLOW_TRACK_SCRIPTS_STATIC_OBJECTS`,
  `ALLOW_TRACK_SCRIPTS_DYNAMIC_OBJECTS` [dll 0x22483b8..0x2248438]; section `_EXTRA_PERMISSIONS` [dll 0x2248460].
- The hooked `SurfacesManager::loadSurfaceDefinitions` (acs.exe 0x1401afad0, RVA 0x1122350, decompiled) calls the
  original, then indexes every `SURFACE*` section by its `KEY` and merges "surface tweaks" (default record for a
  4-letter key, floats 1.0, 0.8, 0.6, 0.05, 0.001, 0.01, 0.15 [dll]; [log-csp] "Loading surface tweaks: 21" for Spa).
- `ac.SurfaceExtendedType` (10): `Base` 0, `ExtraTurf` 1, `Grass` 2, `Gravel` 3, `Kerb` 4, `Old` 5, `Sand` 6, `Ice` 7, `Snow` 8,
  `Wall` 9 (`ac.StateWheel.surfaceExtendedType`); the library `shared/sim/surfaces` adds `raycastDetailed` (type, key,
  section, `dirtAdditive`, `grip`, `damping`, `isValid`, `isPitline`, `blackFlagTime`, `sinHeight`, `sinLength`,
  `vibrationGain`, `vibrationLength`, `wav`, `wavPitchSpeed`, `wearMultiplier`) and `propertyAccessor`.

**Surfaces FX** (`surfaces_fx.ini`, off by default; "currently in development and not yet available online; requires
custom physics"): custom offroad physics for gravel and ice; the tyre model adds the `_SFX_*` parameters (3.2.5); needs
sound banks `extension/sfx/csp_surface_ice.bank` and `csp_surface_gravel.bank`; per-session veto "Surfaces FX:
enabled" in the session information; `DISALLOW_SURFACESFX`; new-mode key `ICE_EVERYWHERE`. Old ice tracks are
classified by an embedded table `internal/configs/surfaces_override.ini` (read from the DLL's overlay archive 7): **14
track ids** (`igi_max`, `simtraxx_karlstad_0.7`, `btbfin_kuadonvaara`, `montecarlostage3`, `snowrally`,
`lillehammer`, `Ice_Ring`, `bozu`, `gokart_center_thessaloniki_winter`, `wdt_redring_winter`, `brivkalni`,
`montagne_de_poudre`, `viragv01_with_garages`, `ek_akina_snow`) each with `ICE=` / `SNOW=` surface-key filters such
as `'{ ! GRASS }'`, `ROAD`, `ASPH, road`. **None of these 14 tracks is installed here.**

**Installed tracks that use CSP track physics** [direct read of `content/tracks/*/**/data/surfaces.ini`; the
`content_scan.json` field is empty because it tests `WAV`, not `WAV_PITCH`]:

| Track | Layout folder | `WAV_PITCH=extended-0` | `[_SCRIPTING_PHYSICS]` |
|---|---|---|---|
| `ks_barcelona` | `layout_moto_fa_2026` | line 6, `[SURFACE_0]` | `ALLOW_APPS=1` (line 192) |
| `ks_red_bull_ring` | `layout_gp_fa_2026` | line 6 | `ALLOW_APPS=1` (240) |
| `ks_silverstone` | `gp_fa_2026` | line 6 | `ALLOW_APPS=1` (256) |
| `montreal` | `montreal_fa_2026` | line 6 | `ALLOW_APPS=1` (113) |
| `monza` | `monza_fa_2026` | line 6 | `ALLOW_APPS=1` (304) |
| `spa` | `spa_fa_2026` | line 6 | `ALLOW_APPS=1` (288) |

For `spa_fa_2026` the only differences from plain `spa/data/surfaces.ini` are `[SURFACE_0] WAV_PITCH` 0 to `extended-0`
and the added section (diff). All other tracks (20 of 26) have no extended-physics key; none has `RIGID_*`,
`CUSTOM_RAYCASTER`, `_VERIFY_INTEGRITY` or `GRAVITY` in any `.ini` (grep). `FF_EFFECT` in `surfaces.ini` is a stock
key unknown to this scan's vocabulary (25 tracks), not a CSP key. The `config/tracks/loaded` files of CSP for the
owner's tracks contain no physics section.

---

## 4. Files it reads

| File | Read by | Purpose |
|---|---|---|
| `<AC>/acs.pdb` | attach (not this area) | symbol names of the 1,542 functions |
| `content/cars/<car>/data/car.ini` `[HEADER] VERSION`, `[_EXTENSION]`, `[CONTROLS]` ... | `INIReader::getInt` hook + car loaders | switch, extension flags |
| car data: `suspensions.ini`, `tyres.ini`, `engine.ini`, `drivetrain.ini`, `brakes.ini`, `aero.ini`, `colliders.ini`, `setup.ini`, `electronics.ini`, `ers.ini`, `dampers.ini`, `bumpstops.ini`, `knc.ini`, `sfp.ini`, `ext_car_controls.ini`, `script.lua`, `script_render.lua`, `script_params.ini`, `ctrl_*.ini`, `*.lut`, `*.2Dlut` | CSP physics loaders (3.2) | |
| `content/tracks/<track>/data/surfaces.ini` (+ per-layout copies) | `INIReader::getFloat` hook, `SurfacesManager` hooks, track physics loader 0x94dcd0 | switch, permissions, track options |
| `extension/config/general.ini` etc. (+ `Documents/Assetto Corsa/cfg/extension/*.ini`) | settings loaders 0x1387350 / 0x138ded0 | 3.1 |
| `extension/config/cars/**`, `extension/config/tracks/**`, `content/*/*/extension/ext_config.ini` | config loader 0x14900d0 (reads `ext_config.ini`, `csp_config.ini`, `SFP_TYRE_MODEL`) | visual and a few physics-adjacent keys |
| `extension/lua/new-modes/*/manifest.ini` | new-mode loader (~RVA 0x2269cd8..0x226a190) | `FORCE_EXTENDED_PHYSICS`, `ICE_EVERYWHERE`, `ALLOW_PHYSICS_ALTERATIONS` |
| `extension/internal/lua-shared/physics/*.lua`, `sim/surfaces.lua` | `require('shared/...')` from car / track scripts | helpers (3.4, 3.5) |
| DLL overlay: `internal/configs/surfaces_override.ini`, `alt_sfp1.ini`, `internal/lua-module/plugins/{stalling,extra_physics_buttons,ai_tweaks,hotlap}.luac`, `config/data_track_params.ini` (435 sections: latitude, longitude, time zone, heading; no altitude) | CSP | embedded defaults |
| `Documents/Assetto Corsa/cfg/extension/state/lua/shared-<car>` | car scripts | the car script's published signal table [prior] |
| setups `Documents/Assetto Corsa/setups/<car>/**` with `[__EXT_PATCH] VERSION=` | setup loader | marker only |

---

## 5. How it is switched on

### 5.1 Switches (deliverable 1)

| # | Switch | Where | Value here | Effect | Evidence |
|---|---|---|---|---|---|
| S1 | car asks | `car.ini [HEADER] VERSION=extended-N` or `extendedV2-N` | 4 cars (+1 with `_EXTENSION` and a script) | per-car extended physics (script, data extensions, COSMIC, extended contact points) | 2.2 |
| S2 | track asks | `surfaces.ini [SURFACE_n] WAV_PITCH=extended-N` | 6 layouts (`*_fa_2026`) | extended track physics ("Extended physics: track"; `TWEAK_TRACK_OBJECTS`, track rigid bodies) | 2.2, 3.5 |
| S3 | track allows scripts | `surfaces.ini [_SCRIPTING_PHYSICS] ALLOW_*` (8 keys) | `ALLOW_APPS=1` on the 6 layouts | `physics.allowed()` true for that script kind | [sdk] [dll] |
| S4 | user forces | `general.ini [PHYSICS_EXPERIMENTS] EXTENDED_PHYSICS=1` | 0 (no file) | every car and track counts as extended (cars without data still run stock data) | 2.2 |
| S5 | new mode forces | manifest `[TWEAKS] FORCE_EXTENDED_PHYSICS=1`, `ICE_EVERYWHERE=1`, `[RULES] ALLOW_PHYSICS_ALTERATIONS=1` | not selected (3 modes force, 5 allow alterations) | extended physics + ice for frozen-drive, p2p-1v1 (when frozen drive set), splitscreen | [lua] manifests |
| S6 | session / server | session-information flags "Extended physics: cars / track / cars & track", `DISABLE_RAIN_PHYSICS`, `SURFACES_FX`, `ICE ...` | n/a offline | [interp] server config | [dll 0x2249cd8..] |
| S7 | experiments gate | closed online, special event, SIMRACERS, RSRLIVETIME, session settings, `FORCE_BASE_PHYSICS` | open offline | all `[PHYSICS_EXPERIMENTS*]` keys ignored when closed | 2.4f |
| S8 | per-feature | `surfaces_fx.ini [BASIC] ENABLED` (0), `rain_fx.ini ENABLED` (1), `STALLING` (0), `ffb_tweaks [REAL_FEEL] ENABLED` (0), `REAL_FEEL_BRAKE` (0), `[GYRO2]` (0), `gamepad_fx [JOYPAD_ASSIST] ENABLED` (0), `[PHYSICS_EXPERIMENTS] ENABLE_MESH_COLLIDER_SOONER` (1), `EXTRA_CONTACT_POINTS` (1) | defaults | see 3.1 | [ini] |
| S9 | car script allowed | `ALLOW_CPHYS_SCRIPT`, `CPHYS_PRO`; `SFP_TYRE_MODEL` blocks cphys scripts | - | [interp] | [dll] |
| S10 | master | `general.ini [BASIC] ENABLED=0` (CM "Active") disables CSP entirely, hence every item of 5.3 | 1 | | [ini] |

Defaults on this PC [prior 2.2, `cfg/extension` listing]: no `general.ini`, `gamepad_fx.ini`, `ffb_tweaks.ini`,
`surfaces_fx.ini`, `tyres_fx.ini`, `small_tweaks.ini`; the owner's overrides are visual only. The owner's current
logs (2026-10-12, `rss_formula_hybrid_2020` at Monaco 2017) confirm: "Cphys Drivetrain Initialized." and "CPHYS ERS not
enabled; initialization skipped." appear for a non-extended car, `ODE_double_precision` is printed, "Pit altitude for
current track: 8", "Pit Y coordinate: -13.3419" [log-csp].

### 5.2 Which installed content is extended (this PC)

| Kind | Items |
|---|---|
| cars with `VERSION=extended-N` / COSMIC | `vrc_formula_alpha_2025_csp`, `vrc_formula_alpha_2026_csp`, `vrc_formula_beta_2024_csp` |
| cars with a physics script and extended data but DWB | `vrc_formula_lithium_2023_csp` |
| car with a physics script and protected data | `urd_darche_992_23` |
| tracks | 6 layouts of `ks_barcelona`, `ks_red_bull_ring`, `ks_silverstone`, `montreal`, `monza`, `spa` |
| everything else (199 cars, 20 tracks) | stock data under CSP's always-on changes |

### 5.3 Always / needs extended physics / needs the car's script (deliverable 7)

Verification column: "ini" = the shipped comment, "dll" = decoded hook / bytes, "log" = owner's log. At least the ten
most important are verified; additions the earlier survey missed are marked **new**.

| # | Behaviour | Class | Evidence |
|---|---|---|---|
| 1 | Air density = barometric formula with altitude (base altitude + car Y), temperature and a near-ground blend, replaces `1.2922 - 0.0041 T` in the aero path | **always** (any car whose avatar has the conditions object) | dll 2.4(a,b); data [prior] **new: formula decoded** |
| 2 | Engine sees the same density through a thread-local override (stock `rho * 0.8263`); constant 1.1839 when a flag byte is set | **always** | dll 2.4(c) **new** |
| 3 | Double-precision ODE core with libccd, multithreaded collisions and joints | **always** | dll 2.4(d), log |
| 4 | Physics threading: acs.exe told "LOGICAL CPU DETECTED: 0" (single-threaded pool), CSP workers (7 on this PC) | **always** | log-ac / log-csp, hook table |
| 5 | Tyre contact point search with Intel Embree instead of ODE rays (`ODE_BATCH_RAYCAST=0`) | **always** (stock cars too) [interp: `Tyre::step` hook not read]; extra points `EXTRA_CONTACT_POINTS` need extended physics | ini + log |
| 6 | `[PHYSICS_EXPERIMENTS_DONE]` fixes (rim collision, smooth / adaptive hard collisions, alt collision handling and processing, sliding fix, brakes limit, optimize rotation, smooth controls locking, randomize wheel angles, alt optimal brake, ground colliders, surface types) | **always, offline** (gate 2.4f) | ini "enabled everywhere now" + dll gate |
| 7 | Mesh collider "sooner", clamp, extend boxes, one-sided boxes, wheel-car contacts, convex colliders | **offline, section hint: cars with custom physics** | ini hint; dll gate |
| 8 | Surface noise changes (`TYRE_*`) on sinusoidal / dirt-additive surfaces | **always, offline**; none on asphalt, kerbs | ini + [prior] |
| 9 | Virtual rain compound appended to every car; `AUTOSELECT_RAIN_TYRES`; RainFX wet grip and racing line | **always** (RainFX on); rain only matters with rain weather | log-ac, ini |
| 10 | `SurfacesManager` surface tweaks, FIX_SURFACE_TYPES | **always** | dll hook, ini |
| 11 | `Car::getGroundWindVector` copies the ground wind to the avatar (graphics) | **always**; no physics change seen | dll decomp **new** |
| 12 | Experiments off online | **gate** | dll bytes **new** |
| 13 | Stalling, REPAIR_WHEELS2, auto-braking modes, AWAKE_IN_PITS, physics shadows | **optional**, stock cars | ini |
| 14 | AI changes (lateral grip estimate, grip multiplier assist, cautious steering, lookahead above 200 km/h, shift thresholds, alternative optimal brake, new smoothing) | **always offline** (`new_behaviour.ini`: "won't affect online") | ini + 12 `AIDriver` hooks |
| 15 | COSMIC suspension, DWB2, extra links, chassis flex | **needs extended physics (car data)** | prior, dll |
| 16 | Aero maps `[MAP_n]`, aero fans, force delays | **needs extended physics** | prior, dll strings "Aero maps (extended physics):" |
| 17 | Tyre thermal model v4, extended raytracing, `TYPE_HINT`, SFX tyre | **needs extended tyre data**; thermal model debug says "This car does not expose an extended tire thermal model" for cars without | dll |
| 18 | Dampers with LUTs / blow-off, bump-stop tables, brake hysteresis and advanced brake system | **needs extended physics** | prior, dll |
| 19 | CPHYS ERS (`[FRONT_MOTORS]`, `[KINETIC]`) | **needs extended physics** (log: "not enabled; initialization skipped") | log |
| 20 | Real-feel steering and brake pedal, clean FFB, extended controls group | **needs extended physics**, wheel hardware | ini |
| 21 | Plank / collider wear | **needs the car's data (`colliders.ini [_EXTENSION]`)** | prior |
| 22 | Surfaces FX (gravel, ice) | **needs extended physics**, module off by default | ini |
| 23 | `SCRIPT_n` controller inputs, `ac.override*`, `addForce`, replaced clutch / brakes / engine torque | **needs the car's script** (`data/script.lua`) | sdk |
| 24 | Everything the VRC Pro cars do (ERS strategy, brake-by-wire, differential maps, DRS logic, fuel, thermal) | **needs the car's script**, encrypted | prior |
| 25 | `physics.*` API (teleport, RigidBody, AI control ...) | **needs a permitted script kind** (track `ALLOW_*`, mode `ALLOW_PHYSICS_ALTERATIONS`) | sdk |

---

## 6. Size

| What | Number |
|---|---|
| physics hook records (`ksPhysicsAC.lib`) | 155 hooks, 82 lookups, 51 classes (ODE core lib: 2 + 1) |
| config files in `extension/config` | 57 ini files, 4,630 active key lines, 198 commented keys; **170 physics-related keys** (general 76, ffb 24, surfaces_fx 11, new_behaviour 33, rain 10, data_manifest 8, small 4, gamepad 2, graphics 1, weather 1) |
| `general.ini` | 470 lines, 35 sections, 264 active keys, 68 commented; `[PHYSICS_EXPERIMENTS]` 26, `[OTHER_EXPERIMENTS]` 12, `[PHYSICS_EXPERIMENTS_DONE]` 20 |
| Lua SDK, car physics (`ac_car_cphys`) | 992 functions (197 own), 64 classes, 95 own `ac.*` physics functions + 31 shared + 13 `physics.*`; `lib.lua` 551,602 bytes, 10,334 lines |
| Lua SDK, track / apps / tools / online | 107 unique `physics.*` (113 declared), `RigidBody` 27 methods; new modes 108; physics worker 31 |
| enums | `ac.CarPhysicsValueID` 29, `ac.SurfaceExtendedType` 10, `ac.Wheel` 9, `ac.ScriptResumeCondition` 8, `ac.PhysicsDebugLines` 6 |
| state structures | `StateCphysCar` 52 fields, `StateCphysWheel` 36, `StateCphysDamper` 6, `StateCphysSurface` 13, `StateCarPhysics` 16 + `StateWheelPhysics` 11 + `StateWingPhysics` 6 |
| shipped physics Lua | 4 files, 24,854 bytes (`lua-shared/physics`) + `sim/surfaces.lua` 3,699 bytes |
| DLL physics string pool | about 520 key-like strings in RVA 0x2242aa0..0x2249e00; settings loaders 0x1387350 (25,641 bytes) and 0x138ded0 (18,589 bytes); car loader 0x32c650 (9,705); tyres loader 0x8cc0e0 (30,533); brakes loader 0x812bb0 (26,500); suspension loader 0x87f080; collider loader 0x9472f0 (10,771); track loader 0x94dcd0 (7,233) |
| DLL RTTI | 25 `cphys::` classes (`ode64_core`, `ode64_rigidbody`, `ode64_dynamic_collider`, `ode_impl_*`, `cphys_suspension_cosmic`, `cphys_suspension_dummy`, `cphys_script::impl::cphys_script_limp`, `virtual_surface`, `data::cphys_values_base`) |
| content using it | 5 cars, 6 track layouts, 14 override tracks (none installed) |

---

## 7. Port difficulty for rustyAC

What rustyAC already has (`docs/port/*`): bit-exact ports of `acs.exe`'s own ODE (`rustyac-ode`, single precision),
DWB / strut / multilink / axle suspensions, the tyre model (v10 and old), aero, drivetrain, 4WD, KERS / ERS (stock),
brakes, ABS / TC, AI driver, collisions with box colliders, wind, pits, race rules, the shared-memory writer, the oracle
tooling (`car_oracle`, `chassis_compare`) that runs `acs.exe`'s own code and compares every step. The
oracle **cannot run CSP**: every proof below is against a real AC + CSP recording, statistical, not bit-for-bit,
because CSP's ODE is a different build (double precision, libccd, threads) and not reproducible bit-for-bit.

Size scale S < M < L < XL as in `docs/atlas`.

| Feature | Size | Reasons | What rustyAC has | How to prove against AC + CSP (record, fields) |
|---|---|---|---|---|
| Air density law (2.4b) | **S** | formula and constants decoded (6 constants); one helper (RVA 0xaf7010: the near-ground temperature blend) and the base-altitude source (`PIT_ALTITUDE` via 0x1126df0) are unread | stock `rho = 1.2922 - 0.0041 T` in `Car::updateAirPressure` port, engine `0.8263 rho` | physics page `airDensity`, `airTemp`, `roadTemp`, graphics `carCoordinates[1]`, `normalizedCarPosition`; compare for a stationary car at 3 altitudes (the formula predicts T_eff 15.2 C at air 14 / road 20); engine: full-throttle accel at 150-200 km/h `rpms`, `speedKmh`, `accG` |
| Always-on collision / contact changes (mesh collider sooner, clamp, rim collision, smooth / adaptive hard collisions, alt handling / processing, ONE_SIDED_BOXES, EXTEND boxes) | **L** | no descriptions; semantics only from machine code of `Car::updateColliderStatus` (RVA 0x59c300), `Car::initColliderMesh` (0x59ce20), `CarColliderManager::loadINI` hook (0x426160), `PhysicsCore::onCollision` lookup; interplay with the double ODE | box-collider contacts, wall / car contacts (`docs/port/contacts.md`) | A/B recording with each key at 0 and 1 (`general.ini`, offline): kerb run: `wheelLoad[4]`, `suspensionTravel[4]`, `rideHeight[2]`, `accG[1]`, `localAngularVel[1]`, `carDamage[5]`; CSP-off run as the reference |
| Double-precision ODE with libccd / threads | **L** | rustyAC's ODE crate is single precision bit-exact with acs.exe's build; a `f64` variant is mechanical but the solver order, libccd contacts and parallel joint solver of CSP's build differ; no bit-exact oracle; tolerance-based acceptance only | `rustyac-ode` (single) | same car and inputs replayed in AC + CSP and rustyAC (`--replay`): per-step `velocity[3]`, `localAngularVel[3]`, `wheelLoad[4]`, `suspensionTravel[4]`, `heading`/`pitch`/`roll`; accept RMS thresholds, drift over 60 s |
| Embree contact points / `EXTRA_CONTACT_POINTS` | **M** | same triangles, tie-break at shared edges and overlapping kerb / asphalt; extra lateral points need the extended tyre loader | exact ray cast (`docs/map/track_surface.md`) | physics page `tyreContactPoint[4][3]`, `tyreContactNormal`, `wheelLoad`, on a kerb sequence |
| AI changes (`new_behaviour.ini` set) | **M** | 12 `AIDriver` hooks + `RaceEngineer` (11); behaviours described only by one-liners | `docs/port/ai.md` bit-exact AI | AI lap recording: `speedKmh`, `gas`, `brake`, `gear`, `steerAngle` per step; compare corner speeds and shift points |
| Track `physics.*` API (teleport, AI control, forces, penalties) | **M** | 107 small functions, each mapped on an existing port call; `RigidBody` needs the double ODE | pit teleports, penalties, forces exist inside the port | per-function Lua test app with shared memory readback |
| `RigidBody` / `addTrackSurface` / track objects | **L** | needs `f64` ODE, semi-dynamic bodies, collision callback, Embree track mesh rebuild | `PhysicsObject` port | Lua app dropping a cone: position vs time |
| COSMIC suspension (bodies, hinges, distance / ball / slider / rack joints with motors, torsion bars, coil-overs, heave, ARB mechanism, flex, K&C) | **XL** | about 200 key names in the DLL, three loaders (0x87f080, 0x811810, 0x870ea0), `cphys::cphys_suspension_cosmic` code not read; ERP / CFM and joint ordering decide the behaviour; depends on the double ODE | DWB / strut / ML / axle | `_csp` car on a flat road and Spa: `suspensionTravel[4]`, `wheelLoad[4]`, `rideHeight[2]`, `camberRAD[4]`; use CSP's virtual K&C tests (`knc.ini`, `/cphys/K&C/` CSVs: heave / roll / lateral / longitudinal / MZ) as a standalone oracle: they output FX, FY, FZ, camber, toe, wheel position per test |
| DWB2 (`USE_DWB2=1` on a DWB car) | **L** | "AC Patch Custom DWB Suspension", own bump-stop model ("new / old bumpstop model") | DWB | as above on `vrc_formula_lithium_2023_csp` |
| Damper LUTs, bump-stop tables, brake hysteresis / advanced brakes | **L** | table formats known from VRC files; semantic order (slow / fast split, thresholds `fastThresholdBump`) from SDK structs only | stock damper / bump stop | `ac.accessCarDamper` fields dump + `suspensionTravel`, `brakeTemp` |
| Aero maps (`[MAP_n]`, 2D LUT, yaw / roll / steer / speed corrections, force delays) | **M** | data format known, delays and corrections from key names | stock wings | `rideHeight[2]`, `speedKmh`, `accG`, CSP Debug "Net CL / CD / balance" at fixed rides |
| Tyre thermal v4, extended raytracing, falloff, SFX | **XL** | 14 + 26 + 17 new keys per compound, layered thermal model, closed code (function 0x8cc0e0 30 KB + hooks `Tyre::step` 2,544 lines, `addTyreForcesV10`) | tyre model v10 and old (bit-exact), stock thermal | `tyreCoreTemperature[4]`, `tyreTempI/M/O[4]`, `wheelSlip[4]`, `fx` / `fy` / `mz` per wheel, CSP Debug tyre thermal panel (TMv4 layers) |
| CPHYS ERS, brake-by-wire, differential and gearbox logic of the VRC cars | **XL, not portable** | entire behaviour is in the encrypted Lua script | stock ERS, KERS | only black-box via `kersCharge`, `kersInput`, `rpms`, `gear`, `brakeBias` |
| cphys Lua car scripts (open ones) | **XL** | embed LuaJIT with FFI-like structs, 95 + 31 + 13 functions, `ScriptData`, timers, `ac.connect`, replay streams, setup items; at least a subset (accessCarPhysics, override*, replaceClutch, setExtraMass, drivetrain setters) is needed for the shipped helpers; the VRC scripts are encrypted and fingerprint-bound | none | Lua test cars written for the purpose; compare to AC + CSP with the same script |
| Surfaces FX (gravel, ice) | **L** | off by default and not online; tyre model parameters `_SFX_*`; sound banks | none | ice / gravel test track: `wheelSlip`, `speedKmh`, `finalFF` |
| Physics worker, `startPhysicsWorker` | **S** | a second Lua state ticked at 333 Hz | none | `ac.connect` echo test |
| Custom raycaster modules (`CUSTOM_RAYCASTER`) | **XL / skip** | native third-party DLLs, integrity-checked | none | none installed |

Cheapest proofs to build first: (1) the air-density law (S) because a recording exists; (2) a **K&C test run** under
CSP gives a clean external reference for COSMIC without any recording logistics; (3) A/B runs of
`ENABLE_MESH_COLLIDER_SOONER` / `ODE_BATCH_RAYCAST` (R3 of the earlier survey).

---

## 8. Open questions and not mapped

1. **What `V2` in `extendedV2-` means** and whether anything reads N later. Both prefixes take the same path in the two hooks.
2. **Where `Script.update()` runs relative to the stock components** inside `Car::step` / `stepComponents` (hooks 0x5996f0, 0x599ec0; the Lua call is not visible).
3. **The near-ground temperature helper** (RVA 0xaf7010, with at least one further chunk) and the **source of the base altitude** (`PIT_ALTITUDE`, 0x1126df0): where 418 m for Spa comes from is in no text file.
4. **Which key lives in which file** for about 100 DLL key names (3.2): attribution by neighbouring strings only; the `engine.ini` keys of the VRC cars (`HUMIDITY_REF`, `AMBIENT_EFFECT`, ...) and the `tyres.ini` `[_EXTENSION]` keys are **not** in the DLL string pool, so they are either hashed / obfuscated or read by the car's script.
5. The meaning and defaults of `ALT_DEPTH_FOR_CAR_BOX_VS_TRACK`, `TOE_ROD_THING`, `DIFF_THING`, `LIMIT_CONTACTS`, `ALLOW_CPHYS_SCRIPT`, `CPHYS_PRO`, `THERMAL2_*`, `GENERATE_EXTRA_TYRES`, the 14 `_SFX_*` values.
6. Whether Embree contact search is also used for cars **without** extended physics (hook `Tyre::step` 0x6ac2a0 not read by this file).
7. The experiments gate decode names two session-object bytes (+0x11, +0x16) and one global (0x26950aa) by use; which is "online" vs "special event" is by the log texts only.
8. What a server must send to enable extended physics online (session information strings exist; the TCP extras are the multiplayer brief's).
9. The encrypted car scripts were not touched; what the `urd_darche_992_23` data holds is unknown; the three `_csp` cars cannot run in plain AC or in rustyAC.
10. The SFP (SFP1) tyre model: configs `sfp.ini`, Lua modules `alt_sfp1*.luac` exist in the DLL; no installed car uses it (no `sfp.ini` seen); behaviour unknown.
11. `Car::getCGHeight`, `Car::computeRideHeight`, `Car::getSteerFF` hooks: effect on shared-memory values not read.

---

## Evidence

- `extension/config/general.ini`, `ffb_tweaks.ini`, `surfaces_fx.ini`, `new_behaviour.ini`, `small_tweaks.ini`, `rain_fx.ini`, `weather_fx.ini`, `gamepad_fx.ini`, `graphics_adjustments.ini` (lines 137-140), `data_manifest.ini` (lines 60-70, 124-133), `tyres_fx.ini`, `track_adjustments.ini`.
- `extension/internal/lua-sdk/ac_car_cphys/lib.lua` (lines 9319-10334, 4492-4502, 6513-6558), `ac_track_script/lib.lua` (10871-11052, 14139-14671, 16534-16560, 4140-4170, 7698-7702), `ac_apps/lib.lua` (8052-8067, 4890-5260), `ac_background_pw/lib.lua` (9655-9778), `ac_new_modes/lib.lua` (12739), `rules.json`, `readme.txt`.
- `extension/internal/lua-shared/physics/*.lua`, `sim/surfaces.lua`; `extension/lua/new-modes/*/manifest.ini`; `extension/internal/lua-apps/CspDebug/{CspDebug,CarDebugWorker}.lua`; `apps/lua/ext_controls/src/UI/pages/sdk.lua`.
- `content/tracks/*/{spa_fa_2026,...}/data/surfaces.ini` (6 files) and `content/tracks/spa/data/surfaces.ini` (diff).
- `re/scratch/csp1/`: `csp_hook_table.tsv`, `dll_strings.tsv`, `dll_func_strings.tsv`, `dll_funcs.tsv`, `dll_zips_all.json` (overlay entries `internal/configs/surfaces_override.ini`, `alt_sfp1.ini`, `config/data_track_params.ini`), `content_scan.json`, `decomp_hooks/{5f0650,5f1110,596f80,597210,68ab90,68b580,5cbdc0,6216c0,5cacb0,1122350,5996f0}.c`, `dwrite_copy.dll` bytes at RVA 0xaf7760 (379), 0xaf7010, 0x918c80, 0x32ca13..0x32cab8, 0x138c026..0x138c20d, 0x138f89a..0x138f91f.
- `prior/csp_survey.md`, `prior/alpha2026_csp.md`, `prior/alpha_keys_csp_only.txt`; `docs/map/{aero,telemetry,car_step}.md`; `docs/port/{suspensions,awd_ers}.md`.
- `Documents/Assetto Corsa/logs/custom_shaders_patch.log`, `log.txt` (2026-10-12 session).

## Decompile requests

| RVA | Question |
|---|---|
| 0xaf7010 (+ later chunks of the same function) | the near-ground temperature helper `Z(roadTemp, ambientTemp, |PhysicsEngine+0x164|, groundHeight)`: confirm the blend factor (0.2?) and what `+0x164` is |
| 0x1126df0 | how `baseAltitude` is computed / where `PIT_ALTITUDE` comes from (418 for Spa, 8 for Monaco); who writes the two floats at RVA 0x268c430 / 0x268c48c |
| 0x5f0170 | the car "extended" flag setter: which object field, which consumers read it |
| 0x9471a0 | the track "extended" flag setter: consumers (TWEAK_TRACK_OBJECTS, track rigid bodies) |
| 0x68acd0, 0x68abe0 | CSP's `PhysicsEngine::step` body: order of cars, joints, collisions, script calls |
| 0x599ec0 | `Car::stepComponents` hook: where the cphys script update and extended components are called |
| 0x59fdb0, 0x59fe60 | `Car::pollControls` hooks: how `ac.accessCarPhysics` control writes (gas, brake, steer, gearUp) are merged with the input |
| 0x7f8ae0 | extension-flag reader (engine / drivetrain / suspension / tyres / assists / car): full key list and defaults (`TORQUE_MODE_EX`, `FIX_PROGRESSIVE_RATE`, `PYTHON_TORQUE_ALLOWED`, `_EXTENSION_AUTOCLUTCH`) |
| 0x87f080, 0x870ea0, 0x811810 | COSMIC / suspension, car controls (power steering) and chassis-flex loaders: joint creation order, ERP / CFM defaults |
| 0x8cc0e0 | tyres.ini loader (30 KB): extended keys and defaults, thermal model versions |
| 0x812bb0 | brakes loader (26 KB): the hysteresis controllers and `USE_ADVANCED_SYSTEM` |
| 0x9472f0, 0x683eb0 | colliders loader and collision-parameter loader: capsule / cylinder, curves, rigid flags |
| 0x94dcd0 | track `_EXTENSION` loader: GRAVITY, RANDOMIZED / ALIGNED_CARS_POSITIONING, integrity check |
| 0x15c9db0 | custom raycaster loader (1,426 bytes): module contract |
| 0x1387350, 0x138ded0 | settings loaders: which section each hidden key (`LIMIT_CONTACTS`, `INTEL_EMBREE_ROBUST`, `FORCE_BASE_PHYSICS`) is read from |
| 0x6ac2a0 | `Tyre::step` hook (2,544 lines): Embree use for stock cars, extended contact points, SFX |
| 0x6b40e0 | `Tyre::addTyreForcesV10` hook: what differs from stock |
| 0x5bba80, 0x5bf3f0 | `Drivetrain::step` / `step4WD_new` hooks: script torque, differential extension keys |
| 0x59c300, 0x59ce20, 0x426160 | `Car::updateColliderStatus`, `initColliderMesh`, `CarColliderManager::loadINI`: the mesh-collider-sooner condition |
| 0x441c80 | CPHYS ERS init: `[FRONT_MOTORS]`, `[KINETIC]`, `[_EXTENSION]` keys |
| 0x839d70 | differential extension loader: `DIFF_FRICTION_FACES`, `DIFF_RAMP_*`, `DIFF_PRELOAD_EXT` |
| 0x97fb20 | session-information function: how "Extended physics: cars / track" is decided |
| 0x14900d0 | per-car config loader: `SFP_TYRE_MODEL`, `sfp.ini`, `csp_config.ini` |
