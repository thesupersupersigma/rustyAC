# Tasks C4a / C4b: CSP physics in `csp_oracle`, CSP's air density

Plan row **CSP-2** ([plan.md](plan.md)). CSP target 0.3.2-preview2, build 4305 ([version.md](version.md));
every `dwrite+0x…` address below is build 4305's, acs addresses are the game's virtual addresses.
CSP mode is defined in [port_c3.md](port_c3.md): physics worker count 0 and
`[PHYSICS_EXPERIMENTS_DONE] RANDOMIZE_WHEEL_ANGLES=0`.

## Resume here

> Updated after each commit.

| Step | State |
|---|---|
| 0. leftovers (areas_test lock, `StepInput::to_bytes`, car short name) | done, f20f03f / 6d64b98 / 71cb25e |
| A. `csp_oracle --drive`: the game's car stepped with CSP live, repeatable, threads check | done, f61c273d; redone in C (the world did not step under CSP, see below) |
| B. the CSP-2 items: addresses, what changes, defaults, plain-session effect | done (table below) |
| C. CSP air density ported under `--csp`, proven bit-exact against CSP's own functions | done (part C below) |
| Finish: quick run, clippy, README features row (no release: decision below) | done: `run_all.sh --quick` 14 areas all identical (0h57m), csp alone 49/49 after the proof-script fix; clippy rule 24 clean in all seven builds |

Decision (lead, during C4a): no whole-car comparison and no release in C4a. `--csp` physics is CSP's
air density on Vanilla's ODE until CSP's own ODE core (`cphys::ode64_core`, double precision, Embree
tyre rays) is ported; the whole-car proof waits for that port.

## Part A: the game's car stepped with CSP live

`csp_oracle <ac root> <run folder> <cfg folder> --car <car> --drive <file.ryin> [--steps n] [--cars n]
[--workers n]` builds the engine, car_oracle's flat road and `n` cars exactly as the car stage does
(CSP loaded, its patches live), gives each car car_oracle's controls device (the
`KeyboardCarControl` vtable with our `acquireControls`, which plays the `.ryin` records), the same
aids, spawn and damage reset, then calls `PhysicsEngine::step` (acs 0x140264760, CSP's detour
`dwrite+0x67ede0`) with dt 0.003 and the clock of the `.ryin`, and after every step
`Car::getPhysicsState` (CSP's detour `dwrite+0x590220`). Per step it writes one line to
`steps.txt`: a hash of every word of every car (pointer words as `ptr`) and of every car's physics
state; at the end every car's words (`car<i>_words_end.txt`). The `.ryin` header must say flat
ground, no pit lane, no stability aid and no wind (it stops otherwise), and no record may carry
events or bias clicks (the oracle has no handler for them).

What CSP needs around a stepped car that the car stage did not (each a shortcut in the code):

| Global (RVA) | What reads it | Stand-in |
|---|---|---|
| `0x2803210` late world step | CSP's `PhysicsEngine::step` (`0x67ede0`) skips the world step while it is set; its drive loop (`0x67ca40`) then steps the world itself after the cars (`core->vtable+0x10(core, 0.003)`, clearing the byte around it) | the oracle does what the drive loop does after every `PhysicsEngine::step` |
| `0x2814e50` physics thread id | CSP's clocks and its "am I the physics thread" tests | the oracle's own thread id, as `PhysicsDriveThread::run`'s one-time set-up (`0x67d6a0`) sets it; its first job (`0x67d8a0`) is called once. Thread name, start time and the set-up's `+122` call are left out |
| `0x267b338` worker count | CSP's physics dispatch | `--workers` (default 0, CSP mode) |
| `0x2820e60` Sim | the GUID-0 pre-step (`0x58a8d0`): pause and ESC menus; clocks: `Sim->game->gameTime` | the zeroed RaceManager stand-in's Sim with a zeroed `Game` (0x278) |
| `0x28213d8` | pre-step byte `+0x2fb`; elsewhere `**g + 0x1168` → Car | a zeroed block whose first word points at a zeroed CarAvatar |
| `0x2820268` TrackAvatar | `TimeTransponder::step` detour (`0x69a3a0`): the track's time lines; `0x58cba0`: the physics track's ray cast | a zeroed TrackAvatar (0x308) with the oracle's Track at `+0xf8` (no time lines) |

The input is `oracle/game/car_random.ryin` (ks_ferrari_f2004, 20,001 steps = 60 s, car_oracle's
smoothly varying random wheel and pedals, flat road). Scripts: `re/scratch/taskC4a/drive.sh <run> <args>`.

**Correction (part C).** The first part A runs (commit f61c273d) never stepped the world: with CSP
the world step is the drive loop's job (`0x2803210` above), so the car's words changed every step
(its own step ran) but the body stood still. Found when the air check showed the car at height 0
for 5,000 steps; the runs below are after the fix. The 3-car runs then crashed in CSP's car
collision callback (`0x587cc0`), which reads `0x28213d8[guid]` for every car: the stand-in now has
one zeroed slot per car (`+0` a zeroed CarAvatar, `+0x440` a zeroed box).

| Case | Workers | Cars | Steps | Result |
|---|---|---|---|---|
| b_a1, b_a2 | 0 | 1 | 20,001 | identical: every step's hash, end words; steps hash d52683c373670927 (32 s) |
| b_w7c1a, b_w7c1b | 7 | 1 | 20,001 | identical to each other and to workers 0 (40-46 s) |
| b_w7c3a, b_w7c3b | 7 | 3 | 20,001 | identical to each other; steps hash 715f9534517a362a (160-219 s) |
| b_w0c3, b_w0c3b | 0 | 3 | 20,001 | identical to workers 7 |

Unlike port_c3's expectation, 3 cars with workers 7 repeat. The cars stand 20 m apart; CSP made its
worker threads (`dwrite+0x342710`), but whether its dispatch put the cars on more than one of them
was not checked. One thing not chased: with CSP, car 0 of three differs from the single car from
step 0 (Vanilla: car 0 of three equals the single car); CSP's collision callback runs for the
three cars, probably on collider poses from before the first sync. CSP mode stays workers 0.

Found on the way: in `csp_oracle` the car's air density (`Car+0xae8`) is the stock fallback
`1.2922 - 0.0041 * 26 = 1.1856`, not CSP's law (part C says why).

## Part B: the CSP-2 items

Addresses: acs function, then CSP's 4305 detour (each found registered by its hook set-up function
in IDA, read in the decompile; earlier notes in [physics_powertrain_aero.md](physics_powertrain_aero.md)
and [physics_car_core.md](physics_car_core.md)). "Plain session": one stock Kunos car alone on a
Kunos track, practice, CSP defaults.

| Item | acs → CSP 4305 | What changes against acs | Key (default) | Plain session? | Task |
|---|---|---|---|---|---|
| Air density | `Car::updateAirPressure` 0x140276ae0 → `0x588850`; law `0xb10410`; near-ground temperature `0xb0fcc0`; altitude `0x118ed80` / `0x118e6c0`, table `0x1237c50` | stock `1.2922 - 0.0041 * ambient`; CSP: barometric law at pit altitude + car height, temperature blended from road and air by height above ground, times a per-car multiplier (Lua `physics.setAirDensityMultiplier`); falls back to stock when the car has no CSP record (`0x28213f0[guid]+0x228`) | none, always | only for a car with CSP's extended physics (part C); a stock car keeps the stock formula | C4a part C |
| Engine thread-local density | `Engine::step` 0x1402880e0 → `0x5bdfa0` | writes the thread-local density (law × multiplier, 1.1839 with `ext+0x6cf`) around the original step; engine torque post-multipliers are 1.0 for stock data | none, always | yes, when the car has a CSP record | C4a part C (density only) |
| `ABS::isInAction` | 0x14028f5d0 → `0x556dd0` (thunk to `0x556d60`) | `speed (Car+0x3e90) > 1 && any absOverride < 0.9`; stock: any `absOverride != 1`, no speed gate | none, always | yes, but it feeds only pedal vibration, the HUD light and sound, not physics | C4b |
| Brake temps while the AI drives car 0 | `BrakeSystem::step` 0x14028e640 → `0x10f15f0` | after the original, GUID 0 with temperature data and `tyres[0].aiMult > 1` (`Car+0x1060`): CSP calls `BrakeSystem::stepTemps`, which acs skips while the AI drives | none, always | only while the AI drives the player's car (session end, auto pit, `--ai-drive`) | C4b |
| `FIX_SURFACE_TYPES` | `SurfacesManager::getSurface` 0x1401af340 → `0x1189b60` | a mesh whose surface lookup fails: skip leading digits, take the longest registered key that prefixes the rest (unless `WALL`) | `[PHYSICS_EXPERIMENTS_DONE] FIX_SURFACE_TYPES=1` (hidden) | only on a track with a mis-named mesh; not checked over every Kunos track (C4b: track_survey can list the failed lookups) | C4b |
| 1 ms transponder | `TimeTransponder::step` 0x1402911f0 → `0x69a3a0`, `onTimeLinePassed` 0x140290c20 → `0x69a0a0` | lap timing per time line from the tyre positions; crossing interpolated between two steps, lap timer adjusted by up to ±3 ms | `PRECISE_TRANSPONDER=1` | yes (lap and sector times) | C4b |
| 250-step AI sleep | `Car::step` wrapper → GUID-0 pre-step `0x58a8d0`, GUID ≠ 0 branch | `framesToSleep` (Car+0x3e28) = 250 for every car but the player's (acs 50): a stopped AI or remote car freezes after 0.75 s instead of 0.15 s | none, always | no (one car) | C4b |
| `getPhysicsState` low-pass | `Car::getPhysicsState` 0x140270d70 → `0x590220` (#2) and `0x58f8a0` (#1) | after the original: tyre block rewrite, accG low-pass 0.25, local angular velocity low-pass 0.02 (flag 0x4), sleeping flag while stationary, state flags | none, always | yes, but only the exported state (shared memory, HUD, apps), never the physics | C4b |

Nothing in the table changes the physics of a stock Kunos car alone on a Kunos track (air density
acts only with CSP's extended physics, part C); air density is the one item that changes the
physics of an extended car alone, which is why part C takes it.

## Part C: CSP's air density, bit for bit

### What CSP does (4305, read in IDA, rule 17: from the code)

- **Who gets it.** `Car::updateAirPressure`'s detour (`0x588850`) and `Engine::step`'s
  (`0x5bdfa0`, which sets the thread-local density `PhysicsEngine::getAirDensity` returns,
  `0x67e1a0`) use the law only when the car has CSP's extended-physics record (`ext`, 0x6f0 bytes,
  made by `0x5e32e0` from the `getInt` detour `0x5e3590`): `[HEADER] VERSION` starting
  `extended-` / `extendedV2-`, or byte `0x280f31e` set (`[TWEAKS] FORCE_EXTENDED_PHYSICS`, or a
  server's `EXTENDED_PHYSICS`). Every other car gets `(1.2922 - 0.0041 * ambient) * multiplier`,
  exactly acs's formula (multiplier table `0x26b9f90`: 256 × 1.0 unless a Lua script sets it).
  So a stock car (the F2004) keeps Vanilla's density under CSP.
- **When.** CSP's `Car::step` wrapper (`0x58afc0`) stores the body position before the original
  step (`ext+0x418`, body `getPosition`) and, after it, the ground plane of the four tyre contact
  points (`0x1150520` into `ext+0x424`); the law inside the step uses the position from before the
  step and the plane from the previous step (zero on the first: height 0).
- **The law** (`0xb10410`): `h = altitude + y` (altitude NaN → `h = 0`); temperature
  `T = Z(road, ambient, |wind speed|, height above the plane)`; `inv = 1 / (T + 273.15)`;
  `x = max(1 - h * 0.0065 * inv, 0)`; `rho = max(powf(x, 5.2557769) * 101.325, 1.1920929e-7) *
  3.4836106 * inv`, all f32 in CSP's order.
- **Near-ground temperature** `Z` (`0xb0fcc0`): road temperature at height ≤ 0, ambient when the
  two differ by less than f32 epsilon; else four rounds of a Monin-Obukhov-style surface layer
  (roughness from `expf`, stability functions from `atanf` / `logf`) and the log profile at the
  height. Two of its constants are one bit above the nearest f32 of their decimal (`-0.04` is
  `0xbd23d70b`, `-0.005` is `0xbba3d70b`): written as bits in the port.
- **Altitude** (`0x118ed80` → `0x118e6c0`, at track load): `[_EXTENSION] PIT_ALTITUDE` in the
  track's `data/surfaces.ini`, else `[ALTITUDE] BASE`, else the built-in table (`0x1237c50`: 26
  Kunos and known track ids, then `0x1236980`: 106 name words and pairs, case-insensitive); minus
  the Y of the `AC_PIT_0` node; an unknown track is 294 m. No installed track has either key.
- **The maths.** CSP's statically linked `expf` / `atanf` / `logf` / `powf`
  (`0x1db8350` / `0x1dc7150` / `0x1dc9b50` / `0x1db47a0`) were run in the oracle over all 2^32
  f32 inputs at the game thread's MXCSR against `rustyac_math::pure`: `atanf`, `logf` identical
  everywhere; `expf` differs only at `-inf` and negative NaNs, `powf(x, 5.2557769)` only for
  negative `x`, neither of which the law can pass.

### The port

`crates/rustyac-physics/src/csp/air.rs`: `near_ground_temperature`, `air_density`,
`ground_plane`, `plane_height`, `known_altitude` (with `GUESSED_WORDS`), `track_altitude`. The
chassis (`ChassisEnvironment::csp_air`) takes the position at the `Car::step` marker and the plane
at the end of the car's step, as CSP's wrapper does; both density users (`update_air_pressure`
and the engine) read it through `ChassisEnvironment::air_density` (`--air-density` still wins).
`GameSim::build_car` sets it only with `--csp` and a car whose `VERSION` is `extended-…`, with the
track's altitude (`csp_altitude`: the two keys, the table, `AC_PIT_0`'s world Y; flat road: 0).
Without `--csp` nothing changes: `csp_air` stays `None` and every path is the old one.

Not ported (shortcuts in the code): `FORCE_EXTENDED_PHYSICS` / server `EXTENDED_PHYSICS`; the
per-car Lua multiplier (always 1); the 1.1839 constant of `ext+0x6cf` (set by no data rustyAC
loads); CSP's slipstream formula (`0x588850`, differs from acs's only with a car ahead: C4b); the
"old CSP track config" test in the unknown-track rule (taken as false).

### The proof

`csp_oracle --drive car_random.ryin --air <track>:<ambient>:<road>:<wind> [--still]` drives the
F2004 60 s with CSP live (part A) and, at every step, gives the car a CSP record stand-in (its
position, a RaceEngineer, the CSP vector slot around the calls only: a registered record makes
CSP's whole extended car step run, which needs CSP's track config), calls CSP's own law, height
hook, `Z` and plane functions with the car's state and the case's conditions, and compares every
result with the port bit for bit. Each run also checks CSP's altitude table against
`known_altitude` for 268 names, `Z` over a grid of 368,640 inputs and the law over 30,492.

| Row (csp area) | Track (altitude) | Ambient / road / wind | Steps | Plane, height, temperature, density |
|---|---|---|---|---|
| `air spa` | spa, 418 m | 14 / 20 / 3 m/s (road warmer) | 20,001 | 0 differ |
| `air ks_black_cat_county` | 1,250 m (highest in the table installed) | 26 / 38 / 0 (calm) | 20,001 | 0 differ |
| `air ks_zandvoort` | 5 m (lowest installed) | 18 / 16 / 6 (road colder: the stable branch) | 20,001 | 0 differ |
| `air still` | spa, 418 m | 14 / 20 / 3, car standing | 20,001 | 0 differ |

The tracks only give the altitude: the car drives on car_oracle's flat road at the track's
altitude (loading a real track in the oracle needs CSP's track config, `0x2821430`, which the
extended-car attempt showed grows without end). The first grid run found 612 of 368,640 `Z`
results off by one or two low bits, all with the road colder than the air: the two constants above.

### What this means for the grip question

[grip_investigation.md](../port/grip_investigation.md) suspect 4 is CSP's thin air: the owner's
CSP lap at Spa recorded `airDensity` 1.157 .. 1.172 against plain AC's 1.2348, and the port's law
gives the same order (about 1.167 at the Spa pits on a 14 C day). But by CSP's code a stock F2004
does not get the law: it needs CSP's extended physics, and neither `FORCE_EXTENDED_PHYSICS` nor a
server key is in this PC's configs. So `rustyac --csp` with the F2004 keeps 1.2348, and the
recording's thinner air has a cause the code read here does not show (open question below). Nothing
was fitted. Question 4 of that page (does the thin air reach the engine) is answered: yes, when the
law applies, through `Engine::step`'s thread-local density.

### Open questions

1. What made the owner's CSP F2004 lap record ~1.166: another way to `ext` for stock cars (the
   `getInt` detour's other callers), a Content Manager or server setting at the time, or a CSP
   version before 4305.
2. Whether CSP's worker threads take the 3 cars in parallel (part A).
3. Why car 0 of three differs from the single car under CSP from step 0 (part A).

### What the whole-car proof waits for, and the next task

A whole-car comparison of `rustyac --csp` against `csp_oracle` needs rustyAC to step CSP's world:
CSP replaces acs's ODE for every car (`ksPhysicsCoreODEFactory::create` detour; the 20 `cphys::`
types: `ode64_core` double-precision world, rigid bodies, colliders, joints, raycaster, the
COSMIC / dummy suspensions, script limp) and casts the tyre rays with Intel Embree
(`ODE_BATCH_RAYCAST=0`). The world step (`0x92e4a0`, string "ODE64: world step") dispatches
through vtables and CSP's job pool, so a static call graph stops at depth 1; the estimate comes
from the type count and rustyAC's own ODE port (`rustyac-ode`, 9,400 lines, float):

| Part | Sessions |
|---|---|
| IDA survey of the 20 `cphys::` types, their vtables and the step order (what is stock ODE in double, what is CSP's) | 1-2 |
| `rustyac-ode` in double precision as CSP built it (quickstep, its seed `0x284dc20`, `MT_ODE_STEP` order, gravity -9.806, surface layer 0.001), bit-checked in-process like part C | 3-4 |
| CSP's contact changes (always-on collision fixes, mesh collider "sooner", hash spaces) | 2 |
| Embree tyre rays (open-source Embree at CSP's version: BVH build and ray-triangle as built, `INTEL_EMBREE_ROBUST=0`) over a real track's mesh | 2-3 |
| `csp_oracle` on a real track (CSP's track config stand-in) and the whole-car rows | 1-2 |
| **Total** | **9-13** |

**Revised in C5a ([core.md](core.md)): 14-23 sessions** (14-20 without Embree). The survey found a
newer ODE than 0.13.1 (threaded quickstep, libccd) and CSP's own collision layer, and that the tyre
rays use Embree only when `ODE_BATCH_RAYCAST` is 1, not 0 as written above; open questions 2 and 3
are answered there.
