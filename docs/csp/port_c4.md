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
| A. `csp_oracle --drive`: the game's car stepped with CSP live, repeatable, threads check | done (this commit) |
| B. the CSP-2 items: addresses, what changes, defaults, plain-session effect | next |
| C. CSP air density ported under `--csp`, proven bit-exact against CSP's own functions | after B |
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
