# Physics extensions (overview)

The part of CSP that decides whether CSP cars drive right. This file is the overview and the index;
the evidence is in four detail files, each written from the shipped text files and from the
decompiles of CSP's replacement functions in `dwrite.dll` (RVAs, image base `0x180000000`):

| File | What it holds |
|---|---|
| [`physics_config_and_api.md`](physics_config_and_api.md) | every switch, every physics config key (170), the data-file extensions of cars and tracks, the physics Lua API (counted), how a car script plugs in, surfaces; the "always / needs extended physics / needs the car's script" table; port difficulty per feature |
| [`physics_car_core.md`](physics_car_core.md) | the hooks in `Car`, `PhysicsEngine`, `PhysicsDriveThread`, colliders, assists; **the physics tick with CSP's injection points**; the per-car extension object |
| [`physics_tyres_suspension.md`](physics_tyres_suspension.md) | `Tyre`, thermal model, `Suspension*`, COSMIC, dampers  |
| [`physics_powertrain_aero.md`](physics_powertrain_aero.md) | `Drivetrain`, `Engine`, `Turbo`, brakes / ABS / TC, ERS, aero, DRS, setup, the ODE core, ray casts, surfaces  |

## 1. The three different things CSP does to physics

1. **Always-on changes for every car**, extended physics or not: the **air density law**
   (standard barometric formula from the track's base altitude plus the car's height and the air temperature with a near-ground blend; humidity and weather pressure are not inputs;
   replacing acs.exe's `1.2922 - 0.0041 T`; the engine sees the same density), **CSP's own ODE build**
   (double precision, libccd, multithreaded collisions and joint solver) instead of acs.exe's
   single-precision ODE, its own physics thread loop (acs.exe is told there are 0 logical CPUs),
   Intel Embree for the tyre contact ray casts, a batch of "enabled everywhere now" collision
   and brake fixes, a virtual rain compound added to every car, changes to the AI driver.
   The collision / contact fixes and the experiments are **offline only** (the gate is closed online,
   in special events and with `FORCE_BASE_PHYSICS`).
2. **Extended physics ("cphys")**, per car or per track, switched by content (below): COSMIC
   suspension and other new data, aero maps, damper / bump-stop tables, a thermal tyre model, brake
   hysteresis, extra differential and drivetrain keys, plus a **Lua script on the physics thread**
   (`data/script.lua`) that adds forces and replaces engine / brake / clutch / differential
   behaviour. A track may allow Lua scripts to use `physics.*`, rigid bodies and extra surfaces.
3. **Offline experiments** (`general.ini [PHYSICS_EXPERIMENTS]`, `[OTHER_EXPERIMENTS]`,
   `[PHYSICS_EXPERIMENTS_DONE]`): optional or already-on fixes; 58 keys.

## 2. How a car or track says "I need CSP" (decoded)

| Switch | Where | Mechanism |
|---|---|---|
| car | `car.ini [HEADER] VERSION=extended-N` (or `extendedV2-N`) | CSP hooks `INIReader::getInt` (acs.exe 0x140235c70, replacement RVA 0x5f0650): strips the prefix, flags the car, returns **N**. `N` is just the number acs.exe would read there; plain AC chokes on the text (`std::stoi`) and refuses the car. The flag is stored at car object +0x914 by CSP's car loader (RVA 0x32c650) |
| track | `surfaces.ini [SURFACE_n] WAV_PITCH=extended-N` (+ `[_SCRIPTING_PHYSICS] ALLOW_*`) | same trick in `INIReader::getFloat` (RVA 0x5f1110), only for files ending in `/data/surfaces.ini`. 6 installed layouts (`*_fa_2026`) use it |
| car data | `[_EXTENSION]` sections in `suspensions.ini` / `brakes.ini` / `bumpstops.ini` / `dampers.ini`, `TYPE=COSMIC`, `[HEADER] ENABLE`, `engine.ini TURBO_n EXT_*`, `tyres.ini TYPE_HINT`, `aero.ini [MAP_n]`, `colliders.ini` wear keys, `ext_car_controls.ini`, `car.ini [_EXTENSION] REQUIRED_VERSION` | ignored by plain AC (unknown keys), read by CSP's loaders |
| user | `general.ini [PHYSICS_EXPERIMENTS] EXTENDED_PHYSICS=1` | every car and track counts as extended |
| game mode | `FORCE_EXTENDED_PHYSICS=1` in a new-mode manifest | frozen-drive, p2p-1v1, splitscreen |
| master | `general.ini [BASIC] ENABLED=0` | CSP inert |

`ABS::step`, `Car::step` and the other hooks test the **existence of a per-car extension object**
(CSP's own struct, created when the car loads), not a bool on the car (see
[`attach.md`](attach.md) section 1.7 and `physics_car_core.md` section 2a).

## 3. The physics tick with CSP

One wake-up of the physics thread runs 1..n steps of 3 ms. CSP replaces the thread loop and
`PhysicsEngine::step` (the order of the stock steps is kept: wind, per-car `Car::step`, ODE
step, post-step), wraps `Car::step` (its own pre and post work around the original), replaces
`Car::stepComponents` (all 29 components, same order as `docs/map/car_step.md`) and runs the
car's Lua `update(dt)` once per step on the physics thread. Section 2b of
[`physics_car_core.md`](physics_car_core.md) holds the ordered list and the table "what
changes at which step". The headline changes at the step:

- every step: the `Car::step` wrapper, `updateAirPressure` replacement (altitude law),
  `updateColliderStatus` post-work, 1 ms transponder crossing correction;
- every step of an extended car: the script's `update`, controller bridge, steering model,
  suspension-damage tracking, snapshot copy;
- other cars (AI / remote) are frozen 250 steps (0.75 s) after standing still instead of acs.exe's rule;
- once per second: acs.exe's `updateBodyMass` (unchanged);
- after each step: `getPhysicsState` post-processing (affects only the exported state, e.g. g-force low-pass).

## 4. What matters for a CSP car to "drive right"

Ordered by how much of the owner's content it decides (details in
[`content_needs.md`](content_needs.md)):

| Rank | Feature | Why | Port size |
|---|---|---|---|
| 1 | Air density law | applies to all 134 owned cars on every track; 5-6 % less downforce and drag at Spa than acs.exe. **Decoded** (constants known) | S |
| 2 | CSP's ODE build and contact handling (always-on collision fixes, mesh collider "sooner", Embree contact points) | every car, offline; effect on kerbs; not bit-comparable with acs.exe | L (statistical proof) |
| 3 | AI changes (`new_behaviour.ini`, 12 `AIDriver` + 11 `RaceEngineer` hooks) | every AI lap differs from plain AC | M |
| 4 | COSMIC suspension + the new `[_EXTENSION]` data | the 3 `_csp` Formula cars cannot load without it | XL |
| 5 | Lua car scripts (LuaJIT on the physics thread, `ac.*` physics API) | the 4 `_csp` VRC cars; their scripts also carry ERS, brake-by-wire, diffs | XL (partly encrypted) |
| 6 | Extended tyre model (thermal v4, extended ray casting, TYPE_HINT) | only extended cars | XL |
| 7 | Aero maps, dampers / bump-stop tables, brake hysteresis | only extended cars (VRC family) | M-L |
| 8 | Track scripting `physics.*`, rigid bodies, extra surfaces | 6 `fa_2026` layouts allow apps; nothing installed uses bodies | L |

## 5. Proof

Recorded real-AC+CSP runs are the only reference (CSP cannot be run by the existing oracles and
its ODE is a different build, so nothing is bit-exact): the air-density law is the cheapest first
proof; a K&C test run under CSP (`knc.ini`) gives a clean external reference for COSMIC; A/B runs of
the single experiment keys (`ENABLE_MESH_COLLIDER_SOONER`, `ODE_BATCH_RAYCAST`) isolate the contact
changes. The per-feature proof table is in `physics_config_and_api.md` section 7 and the plan in
[`plan.md`](plan.md).
