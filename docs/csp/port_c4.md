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
| A. `csp_oracle --drive`: the game's car stepped with CSP live, repeatable, threads check | done, f61c273d; csp area 45/45 IDENTICAL after it |
| B. the CSP-2 items: addresses, what changes, defaults, plain-session effect | done (table below) |
| C. CSP air density ported under `--csp`, proven bit-exact against CSP's own functions | next: oracle makes the car's CSP record, logs the law's inputs and outputs |
| Finish: quick run, clippy, README features row (no release: decision below) | last |

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
| `0x2814e50` physics thread id | CSP's clocks and its "am I the physics thread" tests | the oracle's own thread id, as `PhysicsDriveThread::run`'s one-time set-up (`0x67d6a0`) sets it; its first job (`0x67d8a0`) is called once. Thread name, start time and the set-up's `+122` call are left out |
| `0x267b338` worker count | CSP's physics dispatch | `--workers` (default 0, CSP mode) |
| `0x2820e60` Sim | the GUID-0 pre-step (`0x58a8d0`): pause and ESC menus; clocks: `Sim->game->gameTime` | the zeroed RaceManager stand-in's Sim with a zeroed `Game` (0x278) |
| `0x28213d8` | pre-step byte `+0x2fb`; elsewhere `**g + 0x1168` → Car | a zeroed block whose first word points at a zeroed CarAvatar |
| `0x2820268` TrackAvatar | `TimeTransponder::step` detour (`0x69a3a0`): the track's time lines; `0x58cba0`: the physics track's ray cast | a zeroed TrackAvatar (0x308) with the oracle's Track at `+0xf8` (no time lines) |

The input is `oracle/game/car_random.ryin` (ks_ferrari_f2004, 20,001 steps = 60 s, car_oracle's
smoothly varying random wheel and pedals, flat road). Scripts: `re/scratch/taskC4a/drive.sh <run> <args>`.

| Case | Workers | Cars | Steps | Result |
|---|---|---|---|---|
| a1, a2 | 0 | 1 | 20,001 | identical: every step's hash, end words; steps hash 6cc2769d318a4183 |
| w7c1a, w7c1b | 7 | 1 | 20,001 | identical to each other and to workers 0 |
| w7c3a, w7c3b | 7 | 3 | 20,001 | identical to each other; steps hash b4cd5539ad069246 |
| w0c3 | 0 | 3 | 20,001 | identical to workers 7 |

Unlike port_c3's expectation, 3 cars with workers 7 repeat. The three cars stand 4 m apart and never
touch, so they are three ODE islands with nothing to reorder between them; whether CSP's dispatch
put them on more than one thread was not checked (3 cars took 73 s, 1 car 27 s: about linear).
CSP mode stays workers 0.

Found on the way: in `csp_oracle` the car's air density (`Car+0xae8`) is the stock fallback
`1.2922 - 0.0041 * 26 = 1.1856`, not CSP's law: CSP's `updateAirPressure` detour (`0x588850`) uses
the law only when the car's CSP record (vector at RVA `0x28213f0`, by physics GUID) exists and its
`+0x228` points at the car's state (`+0x320` CarAvatar, `+0x418` position). Nothing in the oracle
makes that record yet (Part C).

## Part B: the CSP-2 items

Addresses: acs function, then CSP's 4305 detour (each found registered by its hook set-up function
in IDA, read in the decompile; earlier notes in [physics_powertrain_aero.md](physics_powertrain_aero.md)
and [physics_car_core.md](physics_car_core.md)). "Plain session": one stock Kunos car alone on a
Kunos track, practice, CSP defaults.

| Item | acs → CSP 4305 | What changes against acs | Key (default) | Plain session? | Task |
|---|---|---|---|---|---|
| Air density | `Car::updateAirPressure` 0x140276ae0 → `0x588850`; law `0xb10410`; near-ground temperature `0xb0fcc0`; altitude `0x118ed80` / `0x118e6c0`, table `0x1237c50` | stock `1.2922 - 0.0041 * ambient`; CSP: barometric law at pit altitude + car height, temperature blended from road and air by height above ground, times a per-car multiplier (Lua `physics.setAirDensityMultiplier`); falls back to stock when the car has no CSP record (`0x28213f0[guid]+0x228`) | none, always | yes (aero and drag of every car) | C4a part C |
| Engine thread-local density | `Engine::step` 0x1402880e0 → `0x5bdfa0` | writes the thread-local density (law × multiplier, 1.1839 with `ext+0x6cf`) around the original step; engine torque post-multipliers are 1.0 for stock data | none, always | yes, when the car has a CSP record | C4a part C (density only) |
| `ABS::isInAction` | 0x14028f5d0 → `0x556dd0` (thunk to `0x556d60`) | `speed (Car+0x3e90) > 1 && any absOverride < 0.9`; stock: any `absOverride != 1`, no speed gate | none, always | yes, but it feeds only pedal vibration, the HUD light and sound, not physics | C4b |
| Brake temps while the AI drives car 0 | `BrakeSystem::step` 0x14028e640 → `0x10f15f0` | after the original, GUID 0 with temperature data and `tyres[0].aiMult > 1` (`Car+0x1060`): CSP calls `BrakeSystem::stepTemps`, which acs skips while the AI drives | none, always | only while the AI drives the player's car (session end, auto pit, `--ai-drive`) | C4b |
| `FIX_SURFACE_TYPES` | `SurfacesManager::getSurface` 0x1401af340 → `0x1189b60` | a mesh whose surface lookup fails: skip leading digits, take the longest registered key that prefixes the rest (unless `WALL`) | `[PHYSICS_EXPERIMENTS_DONE] FIX_SURFACE_TYPES=1` (hidden) | only on a track with a mis-named mesh; not checked over every Kunos track (C4b: track_survey can list the failed lookups) | C4b |
| 1 ms transponder | `TimeTransponder::step` 0x1402911f0 → `0x69a3a0`, `onTimeLinePassed` 0x140290c20 → `0x69a0a0` | lap timing per time line from the tyre positions; crossing interpolated between two steps, lap timer adjusted by up to ±3 ms | `PRECISE_TRANSPONDER=1` | yes (lap and sector times) | C4b |
| 250-step AI sleep | `Car::step` wrapper → GUID-0 pre-step `0x58a8d0`, GUID ≠ 0 branch | `framesToSleep` (Car+0x3e28) = 250 for every car but the player's (acs 50): a stopped AI or remote car freezes after 0.75 s instead of 0.15 s | none, always | no (one car) | C4b |
| `getPhysicsState` low-pass | `Car::getPhysicsState` 0x140270d70 → `0x590220` (#2) and `0x58f8a0` (#1) | after the original: tyre block rewrite, accG low-pass 0.25, local angular velocity low-pass 0.02 (flag 0x4), sleeping flag while stationary, state flags | none, always | yes, but only the exported state (shared memory, HUD, apps), never the physics | C4b |

Only air density changes the physics of a plain single-car session on a Kunos track; that is why
part C takes it.
