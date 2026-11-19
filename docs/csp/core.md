# Task C5a: CSP's physics core, surveyed, and the plan to port it

Plan row **CSP-3** follow-up ([plan.md](plan.md), [port_c4.md](port_c4.md)). CSP 0.3.2-preview2, build
4305 ([version.md](version.md)). Every `dwrite+0x…` offset is build 4305's, relative to the DLL's base.
Nothing is ported in C5a: this page is the map and the plan. Task C5b ran plan parts 1 and 2
([below](#task-c5b-a-real-track-and-three-cars)).

## Resume here

| Step | State |
|---|---|
| 0. release v1.1.1 | done (tag `v1.1.1`) |
| 1. IDA survey + breakpoint log in `csp_oracle` | done (this page) |
| 2. plan | done (last section) |
| C5b 0. stale `cardata/` copy of `rss_formula_rss_3_v6` | done: proof area `csp` says "content changed" |
| C5b 1. `csp_oracle --track` on Spa, repeatability, ray path | done |
| C5b 2. three-car origin contacts | done: car 0 of three = one car |
| C5b plan update | done (Plan) |
| C5c 0. release notes from `docs/releases/<tag>.md` | done |
| C5c A. CSP's ODE release | done: ODE master `66615b8a` (2025-01-05), [below](#task-c5c-csps-ode-release) |
| C5c B. `rustyac-ode` generic, f64 proofs | split into parts 3 (C5c) and 4 (C5d) (rule 32) |
| C5c B0. does CSP call `dWorldStep`? | done: yes, every step; QuickStep never runs for cars ([answer](#does-csp-call-dworldstep-yes-every-physics-step)) |
| C5c B1. `rustyac-ode` generic over the number type | done for rotation and mass: 7 f64 functions bit-identical to CSP's ([below](#part-3-step-1-rotation-and-mass-in-double)); Vanilla unchanged |
| C5c B2a. are CSP's matrix / LDLT loops vectorised? | done: partly, the LDLT / L1 solves are scalar ([answer](#part-3-step-2-vectorised-loops-and-odemath)) |
| C5c B2b. odemath helpers in f64 | done: `dxSafeNormalize3` bit-identical to CSP's; the other helpers have no out-of-line copy in CSP, proven inside their callers ([below](#part-3-step-2-vectorised-loops-and-odemath)) |
| C5c B2c. body integration in f64 | done: `dxStepBody` bit-identical to CSP's ([below](#part-3-step-2c-body-integration-in-double)) |
| C5c B2d. joint rows in f64 | done: ball and fixed `getInfo2` bit-identical to CSP's; CSP has no hinge; ray-vs-trimesh moved to part 8 ([below](#part-3-step-2d-joint-rows-in-double)). Part 3 done |

Next: plan part 4 (C5d): `step.cpp` changes and the new LDLT / LCP solvers first.

## The core in one paragraph

CSP replaces acs's ODE for every car through the `ksPhysicsCoreODEFactory::create` detour
(`dwrite+0x238c80` → creator `+0x6149c0`, a 13,016-byte `ode_impl_core`). Under it is a **full
ODE in double precision, newer than 0.13.1**, built from CSP's own tree (`C:\Development\ode-fresh`)
with trimesh (OPCODE, 16-bit indices, new collider), libccd (internal, cylinder / capsule / convex
pairs), multithreaded collisions and ODE's threading interface but **no** built-in thread pool: CSP
runs islands on its own job pool. The car-side objects are `cphys::` wrappers around ODE bodies,
geoms and joints. Tyre rays go through CSP's raycaster, which uses Intel Embree 3.13.4 unless
`ODE_BATCH_RAYCAST=1` asks for ODE's ray-vs-trimesh (the installed default is Embree, C5b).

## Types (vtables and creators)

The 18 `cphys::` RTTI names are interfaces; the concrete classes live in an anonymous namespace
(`?A0x6f542c96`). A class with no complete-object locator below is a pure interface.

| Type | vtable | Made by | Role |
|---|---|---|---|
| `ode_impl_core` (`ode64_core`, `IPhysicsCore`) | `+0x2304e10`, second interface `+0x2304ed8` | `+0x6149c0` | the world; vtable+0x10 = world step `+0x92e4a0`; second interface: `+0x92ee80`, `+0x92ff50` (create dynamic collider), `+0x92f1a0`, `+0x92f2e0` |
| `ode_impl_raycaster` | `+0x2304eb8` (`+0x35c190`, `+0x92b970` cast, `+0x926900`) | `+0x92fe80` | tyre / probe rays |
| `ode_impl_rigidbody_spec<Base>` / `<SplitAlong>` (`ode64_rigidbody`) | `+0x2305480` / `+0x2304cc0` | `+0x937d90`, `+0x937f90` | body + its geoms (`finalize_dynamic_collider`) |
| `ode_impl_dynamic_collider` (`ode64_dynamic_collider`) | `+0x23052f8` | `+0x926b20`, `+0x92ff50` | moving collision mesh |
| `ode_impl_body_collision_mesh` | shared_ptr block `+0x2304aa0` | | car collision mesh data |
| `ode_impl_collision_{box,plane,cylinder,sphere,capsule,custom,object}` | | | static / track geoms; `get_wall_cobj` dummy `+0x23055d0` |
| joints: ball `+0x23050a0`, slider `+0x2305088`, fixed `+0x23051f0`, distance `+0x23052b0`, angular motor `+0x2305208`, linear motor `+0x2305310`, damper `+0x2304f68`, shifting `+0x2304f80`, repairing `+0x2305148`, pits `+0x2304f58` | | | each over an `ode_impl_*_joint_base` interface (`damper_joint_base` `+0x2304f98`) |
| `ode_impl_ragdoll` (`ode64_ragdoll`) | `+0x2304fb0` | | driver ragdoll |
| `cphys_suspension_cosmic` / `_dummy` | `+0x22d4370` / `+0x22d4450` | `+0x592090` / `+0x591f50` | COSMIC and dummy suspensions |
| `non_joint` | `+0x23019c8` | | no-op joint |
| `cphys_script::impl::cphys_script_limp` | `+0x2301cf8` | `+0x84bcf0` | car-script limp mode |
| `virtual_surface` | `+0x23812d8` | `+0x164b960` | script-made surfaces |

## World set-up (`+0x91c630`, the function that prints ODE's build flags)

| Value | |
|---|---|
| gravity | (0, -9.806, 0) |
| ERP / CFM | 0.3 / 1e-7 |
| contact max correcting velocity | 3.0 |
| contact surface layer | 0.001 |
| quickstep iterations | 48 (SOR left at ODE's default) |
| core fields | +12796 = 50, +12800 = 150, +12816 = 1.0, +12824 = 0.05 |

ODE build string (`+0x23823d0`): `ODE_EXT_no_debug ODE_EXT_trimesh ODE_EXT_opcode ODE_OPC_16bit_indices
ODE_OPC_new_collider ODE_EXT_libccd ODE_CCD_IMPL_internal ODE_CCD_COLL_{cyl_cyl,cap_cyl,box_conv,cap_conv,conv_cyl,conv_conv}
ODE_EXT_mt_collisions ODE_EXT_threading ODE_double_precision`.

ODE version: the `StepIsland Stage*` names match 0.13.1's `step.cpp`, but `QuickStepIsland
Stage4LCP_*`, `ReorderPrep` and `ConstraintsReordering` are not in 0.13.1: they are the threaded
quickstep of the 0.14+ line (0.16.x). There is no version string; the exact release is found in
part 4 by comparing stage functions with the 0.14–0.16.x sources.

## Switches (CSP's `general.ini`, read by `+0x1402bd0`)

| Key | Default | Global |
|---|---|---|
| `MT_ODE_STEP` | 1 | `BYTE2(+0x2803220)` |
| `ENABLE_MESH_COLLIDER_SOONER`, `CLAMP_MESH_COLLIDERS`, `EXTEND_COLLIDER_BOXES` | 1, 1, 1 | |
| `HASH_SPACE_TRACK` (1 / 2 pick two space kinds) | 0 | |
| `HASH_SPACE_CARS`, `ISOLATE_CARS`, `WHEEL_CAR_CONTACTS` | 0 | |
| `TRIMESH_OBB_PREFILTER`, `TRIMESH_AABB_TEST` | 1, 8 | copied into ODE's trimesh globals by the core's creator |
| `ODE_BATCH_RAYCAST` | 0 | `BYTE3(+0x28031fc)` = use Embree: set when the key is 0, or always when the flag `[+0x2821078]+0x16` is set (`+0x13fdd06` in `+0x13fcd80`). C5a had this the wrong way round |
| `INTEL_EMBREE_ROBUST` | 0 | |

The `PHYSICS_EXPERIMENTS` keys are read only for an unlocked session (two config bytes clear and a
third set). Car-level switches (`RIM_COLLISION`, `SMOOTH_COLLISIONS`, `ODE_AFTER_GRAPHICS`,
`PARALLELIZED_CONTACTS`, `FIX_GROUND_COLLIDERS`, …) come through `+0x13fcbb0`.

## Step order

Static graph from `+0x92e4a0` ("ODE64: world step"), confirmed at run time by the breakpoint log
(below):

1. Skip when the late-step byte `+0x2803210` or `+0x27f92b6` is set (C4a: the game's own call is
   skipped; the oracle calls the world step itself).
2. Collision step: pending contact batches (`+0x92c9f0`) else the space collide `+0x92c2b0`
   (near callback `+0x9324a0` → `dCollide` `+0x1687280`, at most 32 / 8 / 16 contacts → CSP's
   contact reduction `+0x169fd90` / `+0x169f060` → contact joints `+0x92c9f0`).
3. `+0x91d670` on the world; the core's callback list; per-item `+0x92b140`.
4. With `MT_ODE_STEP` and workers: CSP's island builder `+0x168d1c0` (in CSP's `ode.cpp`), then the
   job pool `+0x5864f0` runs island jobs `+0x938c50`; the worker count is
   min(islands, threads), cut by a measured-time rule (core +12944). Without: ODE's
   process-islands `+0x16c7650` with the island stepper `+0x16c5850`, then `+0x16c6d00`.
5. Post-step body / joint flags; `+0x168ccc0` on the space.

Cars: `+0x67e2e0` steps each car (`+0x58afc0`) on the same pool when there is more than one car
and workers exist (jobs `+0x67eb80` / `+0x67e6d0` / `+0x67e690`), else in order.

Quickstep's constraint reordering (`+0x16bf520`) is the only user of `dRandInt` (`+0x16c0900`, seed
dword `+0x284dc20`) besides `+0x16c0780`.

### Breakpoint log in `csp_oracle`

`CSP_ORACLE_BREAK=rva,rva,…` (hex DLL offsets; suffix `d` also prints 12 doubles at `rdx`) puts an
`int3` on each address and logs `step thread rva caller rcx rdx r8 r9` to `breaks.txt` beside the
run's `steps.txt`. The steps are byte-identical with and without the log. Results (F2004, flat road):

| Run | What ran |
|---|---|
| 1 car, workers 0 | per step: `67ede0` → `67e2e0` → `58afc0`; world step; `92c2b0`; `91d670`; `16c7650` → `16c5850`; `16c6d00`; `168ccc0`. No contacts, no quickstep, no `dRandInt`, **no raycaster call** |
| 1 car, workers 7 | car step on a worker; world through `168d1c0` → pool → one island job |
| 3 cars, workers 7 | the three car steps on three threads at once |
| 3 cars | 3 contact batches per step near the world origin (x ±0.30, y 0.3–0.67, z ±0.5, depth up to 0.9) and 24–36 calls of the car collision callback `+0x587cc0` across all three cars; the same with the cars moved to x = 100 / 120 / 140 |

## Embree

| | |
|---|---|
| Version | 3.13.4 (string beside "Embree Ray Tracing Kernels") |
| ISAs built | SSE2, AVX (BVH4 + BVH8), AVX2 (only the triangle-mesh ISA and the BVH4 colliders); no SSE4.2 / AVX-512 kernels |
| Tasking | Embree's internal tasking system, device created with `threads=1` |
| Geometry | triangles only; scene quality HIGH; `RTC_SCENE_FLAG_ROBUST` only with `INTEL_EMBREE_ROBUST=1`; meshes added in the order of CSP's mesh list `+0x2885370` |
| Dispatch | at run time, by CPU: best of SSE2 / AVX / AVX2. CSP also picks its packet width from the device (8 if native RAY8, 4 if RAY4, else 1) |
| Used | by the raycaster when `BYTE3(+0x28031fc)` is set, i.e. `ODE_BATCH_RAYCAST` is 0, the default (`rtcIntersect1` in `+0x932900`); otherwise ODE's ray (`+0x926270`) |

**Does a hit depend on the CPU?** Not measured. The AVX2 kernels use FMA and AVX traverses BVH8
where SSE2 uses BVH4, so the last bits of a hit distance, and a tie between two triangles, can
differ between an AVX2 PC and an older one (the proof farm's Westmere has SSE only). A proof must
run the oracle and the port on the same machine, or pin both to one ISA (Embree's `max_isa=`
device option on the oracle side as well).

**Port or link: link.** Embree is Apache-2.0; the car-physics tools are GPL-3.0-or-later, which may
use Apache-2.0 code ([LICENSING.md](../../LICENSING.md), SwiftShader is the precedent). Bit-exact
needs: 3.13.4, the same ISA set (`EMBREE_MAX_ISA`/`EMBREE_ISA_*` set to SSE2 + AVX + AVX2, internal
tasking), `threads=1`, quality HIGH, robust off, same geometry order, float vertices built from the
track mesh exactly as CSP converts them. Cost: a C++ build (CMake, MSVC) inside the tools, and no
browser build (Embree is SIMD C++; the browser keeps the ODE ray path). A Rust port of a HIGH-quality
SAH build with spatial splits, bit-exact, is several sessions on its own and is not worth it while
the default path does not use Embree. Do it only if part 1 shows Embree is on for the cars we prove:
**it is** (C5b: every tyre ray of the F2004 and of the extended `rss_formula_rss_3_v6_2026` on Spa).

Other open-source pieces: ODE itself (BSD-3, `rustyac-ode` already has 0.13.1's `dWorldStep`, LCP,
OPCODE and the box / trimesh colliders in float), libccd (BSD-3, inside ODE: not in `rustyac-ode`),
the newer threaded quickstep (BSD-3/LGPL dual like ODE: not in `rustyac-ode`). All are ported, not
linked, so the browser gets them.

## Answers to C4a's open questions

- **Q2, do CSP's workers take the 3 cars in parallel?** Yes: with workers the three car steps run on
  three threads at once (`+0x67e2e0` → pool). The world's islands also go to the pool. With workers
  0 (CSP mode, [port_c3.md](port_c3.md)) everything is on one thread.
- **Q3, why car 0 of three differs from one car from step 0.** With three cars, contacts appear
  every step at fixed points near the world origin and feed every car's collision callback; one car
  has none. They stay at the origin when the cars are moved away, so they come from geoms that do
  not follow the cars, most likely per-car colliders the oracle's stand-ins leave at their zeroed
  default. **C5b: wrong guess.** The geoms were the cars' own collision meshes, all three left at
  the world origin because CSP's `Car::forcePosition` never ran (section C5b). Fixed in the oracle.

## Plan

Each part ends bit-exact against `csp_oracle` running CSP's own code, the way C4a proved the air
density: same inputs through CSP's function and the port, every output compared as bits.

| # | Part | Sessions | Proof |
|---|---|---|---|
| 1 | `csp_oracle` on a real track (Spa, CSP track config stand-in) with breakpoints on the raycaster: which ray path the cars take, whether `ODE_BATCH_RAYCAST` is forced | 1-2 | **done in C5b** |
| 2 | Q3: owner of the origin contacts, fix the stand-in or document it | 0.5-1 | **done in C5b** |
| 3 | `rustyac-ode` generic over the number type (f32 Vanilla, f64 CSP; the float path's results must not move), then the code that is the same in 0.13.1 and `66615b8a` proven in double: rotation, mass, odemath and matrix helpers that did not change, body integration, ball / fixed rows (**done in C5c**; ray-vs-trimesh went to part 8, the LDLT / solve kernels to part 4: master rewrote them; CSP has no hinge) | 2 used (planned 4-6) | existing ODE proofs unchanged; each ODE function vs CSP's copy in-process, bits |
| 4 | the `66615b8a` code 0.13.1 does not have (C5d): `step.cpp` changes and the new LDLT / LCP solvers (CSP steps cars with `dWorldStep`, [answer](#does-csp-call-dworldstep-yes-every-physics-step)) 1-2; contact `getInfo2` and the box / trimesh colliders that changed 1-2; `odemath` / `matrix` changes 0.5; libccd pairs 1. QuickStep (3,600 lines) only runs for islands whose bodies all carry CSP's flag `0x200`, never seen for cars: later, not counted | 3.5-5.5 | per-stage in-process compare |
| 5 | CSP's world step: island builder, island jobs, worker count rule, post-step flags | 1-2 | `steps.txt` identical at workers 0 and 7 |
| 6 | collision: near callback, contact reduction, contact creation, the switches above, car collision callback | 2-3 | contact lists identical per step |
| 7 | `cphys::` wrappers stock cars use: core, rigid bodies, dynamic collider, joints of the suspensions, raycaster, dummy suspension; and what CSP's loading jobs (AI grids, AI payloads, C5b) hand to the car | 2.5-4 | body states identical after set-up and per step |
| 8 | tyre rays: Embree linked, the default for every car (2-3); ODE ray path for `ODE_BATCH_RAYCAST=1` and the browser (1) | 3-4 | hit points identical |
| 9 | whole-car rows in proof area `csp`, `rustyac --csp` steps CSP's core | 1-2 | rows identical on this PC |
| | **Total left after C5c** | **13-20.5** (parts 4-9; C5b: 15-22; C5c found ODE from 2025, cars on `dWorldStep`, no QuickStep) | |

Later, not counted: QuickStep (bodies with CSP's flag `0x200`; who sets it is an open question), COSMIC, car-script limp, ragdoll, repairing / pits joints, virtual surfaces,
hash spaces (all off or unused for stock cars by default).

### Decisions

- Port ODE, link Embree (needed: C5b found it on for every car). ODE runs in the browser; Embree
  cannot, so the browser's CSP mode takes ODE's ray and is not bit-exact with the desktop's default.
- Prove each part in-process against CSP's own function before the whole car: a whole-car diff
  without them only says "different".
- Proofs on this PC only until a part shows no CPU dependence (Embree, FMA).

### Risks

- The ODE release behind `ode-fresh` may be a CSP-modified tree; parts 3-4 then compare against
  CSP's machine code instead of a source tarball (slower). **C5c:** it is ODE master `66615b8a`
  with at least the island-builder and contact-reduction changes listed there; every other
  function still has to be checked against that commit as parts 3-4 reach it.
- Island jobs on several threads: islands are independent, but CSP's measured-time worker rule
  makes the thread count vary run to run; part 5 must show the result does not depend on it.
- The float-to-double split in `rustyac-ode` could move the float path's last bits; part 3 guards
  with the existing proofs.
- CSP's loading jobs change the car's physics when they land (C5b). Which value they feed is not
  known yet; part 7 finds it.

## Task C5b: a real track and three cars

### Step 0: a stale `cardata/` copy

`rss_formula_rss_3_v6` was reinstalled (2026-11-17) and is no longer a CSP car (`car.ini` `VERSION=1`,
was `extended-1`); the git-ignored `cardata/` copy was the old one, so two `csp` cases said DIFFERENT.
The copy was remade the way it was made (`python tools/acd_extract.py --car <AC>/content/cars/<car>
--out cardata/<car>`) and the car moved to the area's plain list. The area now asks `acd_check
--only <car>` first and reports `SKIPPED | content changed: <car>, refresh cardata/` instead of
DIFFERENT when the install no longer matches the copy.

### Part 1: `csp_oracle --track <folder>`

`csp_oracle ... --track <AC>/content/tracks/spa --drive oracle/game/track_spa_lap.ryin` builds the
track with car_oracle's builder (`tools/car_oracle/src/track.rs`: the game's own `Track`,
`SurfacesManager` and `Track::addSurface` for all 455 physics meshes, ghost as the tape, the three
timing lines) and drives from the tape's spawn with car_oracle's session start setup (tyre compound,
setup screen defaults). Visuals are not loaded. What CSP needed around it:

| Need | Where | Stand-in |
|---|---|---|
| the `Track` inside its `TrackAvatar` at +0xf8, the avatar in the Sim global `+0x2820268` | CSP reaches the track through the avatar | `BuildPlan::in_avatar` |
| the Sim global `+0x2820e60` set before the track is built | AI-line detour `+0x5f1d60` (spline reversal check) reads Sim+0x128 | track built after the stand-ins; zeroed Sim: no reversal |
| track info global `+0x2821430` (+0x68 track name, +0x88 layout) | AI grid cache name in `+0x5f2920` | 0x200 zero block with the two strings |
| `Car::forcePosition` | CSP's hook `+0x58d7d0` teleports at once only off CSP's main thread (`+0x26bada8`, the loading thread); on it the call is queued (`+0x2814d78`) for its `PhysicsAvatar::stepCommandQueue` hook `+0x676470` | the oracle steps on its own physics thread, set up as CSP's `+0x67d6a0` does (FTZ+DAZ, `+0x2814e50`, `+0x67d8a0`), and runs that queue before each step |
| CSP's job pool (workers `+0x572660`, jobs queued or running at pool+696) builds AI grids and AI payloads after loading; when they land the car's tyres change | | the drive starts when every pool is idle (about 250 ms), as the game's loading screen would wait |
| `SurfaceDef` bytes no code sets (wav name after its NUL, padding) differ run to run | copied into the physics state | `BuildPlan::clean_surfaces` zeroes them |
| one physics state buffer per car | CSP's `getPhysicsState` hook `+0x590220` smooths wheel speeds against the buffer's old contents | |

Results (F2004, Spa, 60 s, 20001 steps): two CSP runs at workers 0 give the same `steps.txt` (car
and physics state, steps hash `53ce0e8f5ddf5716`). With `--vanilla` the chassis body matches
car_oracle's own game recording of `spa_lap` in 20001 of 20001 steps. Before the pool wait two CSP
runs split at a random step (35-51 with Embree, 7113 with ODE's ray): the jobs' timing.

**Ray path:** the installed `general.ini` has `ODE_BATCH_RAYCAST=0`, so `BYTE3(+0x28031fc)` = 1 (the
forcing flag is 0). Every tyre ray of the F2004 and of the extended `rss_formula_rss_3_v6_2026`
(4 per step, `Tyre::step` → `+0x92b970` → `+0x932900`) goes through Embree; ODE's ray `+0x926270`
runs once at set-up. With `ODE_BATCH_RAYCAST=1` in the run's `general.ini` the switch is 0 and the
rays take ODE.

### Part 2: the origin contacts

They were the cars' own collision meshes: under CSP `Car::forcePosition` was queued on the main
thread and never ran, so all three cars sat at the world origin inside each other (C5a's runs
too). With the physics thread the cars start at x = 0 / 20 / 40, no contact batch or `+0x587cc0`
call is left, and car 0 of three equals one car in car words and physics state for 2000 of 2000
steps (`car_random.ryin`, workers 0), CSP and Vanilla alike.

## Task C5c: CSP's ODE release

**Answer: ODE's `master` branch at commit `66615b8a` (2025-01-05, "Improve speed, accuracy, and
readability of dCollideBoxPlane"), double precision, plus CSP's own changes.** No tagged release
matches: the 0.16.x tags sit on a side branch and lack every feature below. How sure: high for
the lower bound (two independent body checks), medium-high for the upper bound (one missing
string, which CSP could have removed by hand).

| Evidence in `dwrite.dll` | Oldest ODE commit with it | Newest without it |
|---|---|---|
| strings `Stage4LCP_*`, `Stage6a` sync names | 0.13.2 | |
| asserts `!row1_norm_fault` (odemath), `nj < ~((atomicord32)0) / dJCB__MAX` (step) | 0.15 | |
| world set-up writes QuickStep's dynamic iteration fields (`qs` +172 = `(int)(+176 * 48)`, +200 = "dynamic on") | master `7811250d` (2019-02-21), not in any tag | |
| `dGetConfiguration` lists `ODE_CCD_IMPL_internal` and the `ODE_CCD_COLL_*` pairs | master `197ed3ff` (2020-02-19) | |
| `dWorldCreate` (`+0x168cf90`, 312 bytes): after `body_flags` / `islands_max_threads` come two more `unsigned` (0 = unlimited, then 1) before `wmem`: `stepping_max_threads`, `solving_max_threads` | master `2cebd640` (2024-12-29) | |
| box-vs-plane collider (`+0x169c2c0`, `colliders[1][4]` of the table the init at `+0x1686990` fills) sorts the three projected sides, writes the deepest corner first and goes on past four contacts | master `66615b8a` (2025-01-05) | |
| `dGetConfiguration` has no `ODE_EXT_inelastic_collisions` | | master `aff542d5` (2025-01-14) |

No `ode/src` commit lies between `66615b8a` and `aff542d5`, so the snapshot is exactly one tree.
`ODE_THR_builtin_impl` is missing too: ODE's thread pool was built out, CSP runs islands on its
own job pool (C5a).

### CSP's changes to that tree (known so far)

- Island builder (`+0x168d1c0`): asserts `islandCount <= (sizeint)INT_MAX`, a string in no ODE
  commit on any branch, so CSP edited the function.
- Contact reduction after `dCollide` (`+0x169fd90`, `+0x169f060`, C5a): CSP's own, not ODE.
- Build: `ODE_EXT_no_debug`, double, OPCODE 16-bit indices, no GIMPACT, no built-in threads.

Not done: a function-by-function compare of every ODE function in `dwrite.dll` against
`66615b8a`. Parts 3 and 4 do it function by function as the in-process proofs reach each one; a
differing proof is then either a CSP change or a port bug, and the source of `66615b8a` tells which.

### Size of the gap to `rustyac-ode` (0.13.1 in float)

`git diff -w 0.13.1 66615b8a`, ignoring line endings, per file:

| File | + / - lines | Goes to |
|---|---|---|
| `rotation.cpp` | 0 | part 3 |
| `mass.cpp` | 3 / 3 | part 3 |
| `collision_kernel.cpp`, `collision_trimesh_ray.cpp`, `util.cpp` (islands) | 21-63 each | parts 6, 8, 5 |
| joints `ball` / `fixed` / `hinge` | 9-33 each | part 3 (no hinge in CSP) |
| `odemath.cpp` / `matrix.cpp` | 174 / 71, 211 / 125 | part 3 (`dxSafeNormalize3`), part 4 (the rest, with the solvers) |
| `joints/contact.cpp` | 206 / 198 | part 4 |
| `box.cpp`, `collision_trimesh_box.cpp`, `collision_trimesh_opcode.cpp` | 309 / 26, 169 / 237, 446 / 574 | part 4 |
| `quickstep.cpp` | 2668 / 786 (3,587 lines; `rustyac-ode` has no QuickStep) | part 4 |
| `step.cpp`, `lcp.cpp` + new `fast*_impl.h` LDLT solvers (~4,500 lines) | 620 / 408, 452 / 353 | part 4 (CSP steps cars with `dWorldStep`, below) |

Vanilla AC steps with `dWorldStep` (Dantzig LCP). CSP sets QuickStep's iteration count to 48 in
its world set-up, but does not step the cars with it:

### Does CSP call `dWorldStep`? Yes, every physics step

- Workers 0: the world step `+0x92e4a0` calls `+0x16c7650(world, info, dt, +0x16c5850)` then
  `+0x16c6d00` = `ode.cpp`'s `dWorldStep` body (`66615b8a` line 1879): `+0x16c5850` has the
  `m * dPAD(m)` A-matrix term of `step.cpp`'s `dxEstimateStepMemoryRequirements`, and
  `+0x16c6d00`'s call-count estimator `+0x16c5a10` returns `2 * n + 4` = `step.cpp`'s
  `dxEstimateStepMaxCallCount`.
- Workers > 0: CSP's island builder `+0x168d1c0` picks the estimator per island in `+0x168d180`:
  QuickStep's (`+0x16c1f80`, no m×m term) only when every body of the island has flag `0x200`
  (`+56`), else `step.cpp`'s. `0x200` is CSP's own: ODE's body flags stop at `dxBodyGyroscopic = 256`.
- Breakpoint counts, Spa, `track_spa_lap.ryin`, 600 steps (`re/scratch/taskC5b/runs/c5c_brk_*`):
  F2004 and `rss_formula_rss_3_v6_2026` at workers 0, F2004 at workers 4: `+0x16c5850` 600,
  `+0x16c1f80` 0 in all three.

So part 4's LDLT / LCP item is needed and QuickStep is not, for stock and extended cars.

### Part 3, step 1: rotation and mass in double

- `crates/rustyac-ode/src/common.rs` has `trait Real` (f32 and f64): constants, `sqrt` (f32: the
  runtime's `sqrtf` as before, f64: `f64::sqrt`) and `FAST`. `Vector3`, `Quaternion`, `Matrix3` and
  `Mass` take the number type with f32 as default, so every Vanilla caller is unchanged.
- CSP's ODE is compiled with reassociation (SSE2, no FMA): some sums come out in another order
  than ODE's source and than the decompiler's pseudo-C. Where the order differs the code has
  `if R::FAST { CSP's order } else { ODE's order }`, the f64 branch read from the machine code:
  `dQMultiply1` +0x168b7b0 (`r[0]`), `dRfromQ` +0x168b990 (`r[2]`, `r[4]`, `r[9]` as
  `(q0+q0)*q2 + (q1+q1)*q3` and so on), `dDQfromW` +0x168bca0 (row 2). `dQMultiply0` +0x168b680,
  `dQMultiply3` +0x168b8a0, `dQfromR` +0x168bae0 (`0.5/s`, the same bits as `0.5*(1/s)`),
  `dMassSetZero` +0x168d4f0 and `dMassSetBoxTotal` +0x168d670 keep ODE's order.
- Called during a 600-step Spa drive (both cars, 0 workers): `dQMultiply1` 600, `dRfromQ` 3600,
  `dDQfromW` 3600, mass functions 2 each; `dQfromR`, `dQMultiply0/3` not called.
- Proof: `csp_oracle --ode <breaks.txt>` (`tools/csp_oracle/src/ode.rs`) calls CSP's 7 functions
  in process and rustyac-ode's f64 ones on the drive's own inputs (4200 quaternions, 3600 angular
  velocities from breakpoint dumps), every pair of 25 edge values, random bit patterns and unit
  quaternions, at MXCSR 0x1f80 and 0x9fc0 (flush-to-zero): 1,204,200 inputs each, 0 differ, for
  both cars. With the f64 path planted to ODE's order it finds 119,662 / 59,074 / 1,486
  differences in `dQMultiply1` / `dRfromQ` / `dDQfromW`. Proof area `csp`, cases `ode <car>`
  (quick set, 2m40s).
- Size: 7 functions took a session because each needs its order read from the machine code; the
  matrix and LDLT loops may be vectorised as well. Part 3 is now 4-6 sessions, one done.

### Part 3, step 2: vectorised loops and odemath

**Are CSP's matrix / LDLT loops vectorised?** Partly. Counted over every function of CSP's ODE
(dwrite.dll +0x1680000..+0x1710000, packed-double `addpd/subpd/mulpd/divpd` and horizontal adds
`unpckhpd/haddpd/movhlps` per function):

- Element-wise loops are packed two doubles at a time and unrolled (`a[i] += s*b[i]` @ +0x16ff6b6:
  eight per round with `mulpd`/`addpd`, a scalar tail). Each lane does what the scalar code does:
  same bits, the port can stay scalar.
- Some sums of products are reduced in pairs: +0x16fe3d0 (a solve kernel with two accumulators)
  multiplies with `mulpd`, adds the two packed products with `addpd`, then folds the halves with
  `unpckhpd` + `addsd`, so four products are summed `(p0+p2) + (p1+p3)` before they meet the
  accumulator. That is reassociated: the f64 port must copy this order. 18 functions have such
  horizontal adds (the body / joint / collider code at +0x168d930, +0x169e9f0, +0x16ae450,
  +0x16b77c0, +0x16bd6b0, +0x16bf3b0, +0x16c0100, +0x16c16dc, +0x16c2834, +0x16c5370 and the
  solver code at +0x16fc470, +0x16fe3d0, +0x16fed7b); each gets its order read when it is ported.
- The big LDLT factor and L1 solve kernels (+0x17010c9 / +0x1702789, +0x17017b0 / +0x1702e80, two
  copies each, and +0x1702440) are scalar `mulsd`/`addsd` only, unrolled like ODE's
  `fastldlt` / `fastlsolve` source: no lane reordering, but the unrolled sums still follow the
  compiler's order, read from the machine code.
- Sizing: the vectorised reductions are a few lines each where they occur; they do not change
  the plan (part 3: 4-6 sessions, part 4: 3.5-5.5).

**odemath.** Of master's `odemath.cpp` only `dxSafeNormalize3` has an out-of-line copy
(+0x168aed0; `dNormalize3` +0x168b060 calls it, 40 callers in the colliders and joints).
`dxPlaneSpace`, `dxOrthogonalizeR`, `dxSafeNormalize4` and the header inlines (dot, cross, the
`dMultiply` family) are inlined into their callers; they are proven with those callers (body
integration, joints, colliders), not on their own.

- `dxSafeNormalize3` keeps master's source order: the largest component taken as exactly one,
  `l = 1/sqrt(1 + ti*ti + tj*tj)` (the machine code adds `ti*ti + 1` then `tj*tj`: the same
  bits), the other two `*l`, the largest one `_copysign(l, a[idx])` (CRT +0x1dc98f0). A zero
  vector returns false and is left unchanged (0.13.1 wrote (1, 0, 0)).
- `safe_normalize3` in `crates/rustyac-ode/src/odemath.rs` is generic: `if R::FAST` takes
  master's code, f32 keeps 0.13.1's. `Real` gained `abs` and `copysign`.
- Proof: `csp_oracle --ode` checks both halves of each 8-value input (2,408,400 vectors) with
  the return value: 0 differ at MXCSR 0x1f80 and 0x9fc0. With the f64 path planted to 0.13.1's
  code it finds 476,219 / 542,137 differences. Proof area `csp`, cases `ode <car>`: 16 lines.

### Part 3, step 2c: body integration in double

CSP's `dxStepBody` is +0x16c61b0 (called from the island steppers +0x16c5370 and +0x16c1e80).
It keeps master's `step.cpp` order except in three places, which the f64 path copies:

- The quaternion normalise (master's `dxSafeNormalize4`, inlined at +0x16c67df) is
  reassociated: `l = (q0*q0 + q1*q1) + (q2*q2 + q3*q3)`; `l > 0` gives `q *= 1/sqrt(l)`,
  anything else (zero, NaN) `(1, 0, 0, 0)`. `safe_normalize4` and `dot3` in
  `crates/rustyac-ode/src/odemath.rs` are now generic, `if R::FAST` takes that order.
- In the finite-rotation-axis case the infinitesimal update is inlined (+0x16c664c): row 0
  is `q0 - h*(0.5*(w0*q1 + w1*q2 + w2*q3))`, a subtraction of the positive sum where the
  source adds the negated one. Same value, but an exact-zero sum gives the other zero sign.
  Rows 1-3 match the out-of-line `dDQfromW` (+0x168bca0, row 2 reordered as in step 1).
- `sin` / `cos` are CSP's statically linked CRT (+0x1dbef50 / +0x1dbfb40); Rust's `f64::sin`
  / `cos` give the same bits on every input below.

The geoms loop and the moved callback are skipped when byte `[[[[body+8]+0xa0]+8]+0x28]` is
set; the oracle gives CSP a fake world chain with that byte set, so only the integration is
compared. `Body`, `Damping` and `AutoDisable` in `crates/rustyac-ode/src/world.rs` take
`R = f32`; `step_body` and `sinc` in `crates/rustyac-ode/src/step.rs` are generic; `Real`
gained `ONE_SIXTH`, `sin`, `cos`, `to_f64`. The f32 path is unchanged.

Proof: the Spa drive logs each `dxStepBody` call (breakpoint kind `b`: h, flags and the body
fields); `csp_oracle --ode` runs CSP's function and `step_body::<f64>` on a fake body with
those values under all 32 combinations of the finite-rotation, axis, linear / angular damping
and max-angular-speed flags, plus edge values and random bits: 1,319,392 bodies, position,
quaternion, R, lvel and avel compared, 0 differ at MXCSR 0x1f80 and 0x9fc0. Planted with the
source-order normalise it finds 53,036 / 53,031 differences; before the row-0 fix it found 16 /
38 (zero signs, edge set only). Proof area `csp`, cases `ode <car>`: 18 lines.

### Part 3, step 2d: joint rows in double

**Which joints.** CSP's ODE has the vtables of ball (type 1, `getInfo2` +0x16a3ba0), slider (3),
contact (4), universal (5), fixed (7, +0x16a0fd0), AMotor (9), LMotor (10), PU (13) and DBall
(15); no hinge (type 2) is linked, so there is nothing to prove for it.

- Ball: `getInfo2` writes the joint's cfm (+0xe8) to the three rows and tail-calls `setBall`
  (+0x16c8670) with erp +0xe0, anchor1 +0xa0, anchor2 +0xc0. `setBall` keeps master's source
  order (`k * (a2 + b1.pos - a1 - b0.pos)`, `k = fps * erp`).
- Fixed: `getInfo2` first tests the field +0x110 against -1; -1 runs master's code (qrel +0xa0,
  offset +0xc0, erp +0xe0, cfm +0xf8), any other value CSP's own path (+0x16a1287): part 4.
  `setFixedOrientation` (+0x16c8a10) differs from the source twice: the inlined `dQMultiply2`
  regroups row 0 to `(c3*b3 + c2*b2) + (c0*b0 + c1*b1)` (rows 1-3 keep ODE's order; row 0 only
  feeds the sign test), and the rhs factor is `(erp + erp) * fps`, not `fps * erp * 2`
  (another result on overflow).
- Port: `Info2` is generic; `ball_get_info2` and `fixed_get_info2` in
  `crates/rustyac-ode/src/joint.rs` are public and generic, `q_multiply2` and the rhs factor take
  CSP's order `if R::FAST`. `multiply0_331` and `set_cross_matrix_plus/minus` are generic.
- Proof: `csp_oracle --ode` calls CSP's two `getInfo2` on a joint stand-in with two dxBody
  stand-ins: the drive's 3,600 bodies (rotation from their quaternion, 333 Hz), then each input
  set four values at a time, rotations from the quaternions and raw in turn, every fourth joint
  without a second body. It compares the Jacobian rows (angular part moved from CSP's column 3
  to the port's 4, padding included), rhs and cfm: 1,207,788 joints each, 0 differ at MXCSR
  0x1f80 and 0x9fc0. Plants: ODE's `dQMultiply2` order 18 / 8 differ (edge values only, row 0
  feeds only the sign test), `fps * erp * 2` 1,607 / 881, a regrouped ball rhs 133,845 / 133,758.
  Proof area `csp`, cases `ode <car>`: 22 lines.
