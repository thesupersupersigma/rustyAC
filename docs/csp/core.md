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
| C5d 1.1 solver kernels in f64 | done: master's LDLT factor, L1 / L1T solves, dots, `dxLDLTAddTL`, `dxLDLTRemove` bit-identical to CSP's ([below](#task-c5d-part-1-step-1-solver-kernels-in-double)) |
| C5d 1.2 `dSolveLCP` in f64 | done: `csp_lcp::solve_lcp` bit-identical to CSP's on 96,000 problems ([below](#task-c5d-part-1-step-2-dsolvelcp-in-double)) |

| C5d 1.3 contact rows, fixed joint's other path | done: `csp_contact::get_info1` / `get_info2` bit-identical to CSP's on 1.2 M contacts; the other fixed path decoded, not ported ([below](#task-c5d-part-1-step-3-contact-rows-and-the-fixed-joints-other-path)) |

| C5d 2.1 dball rows | done: `csp_joints::dball_get_info2` bit-identical to CSP's on 1.2 M joints ([below](#task-c5d-part-2-the-car-islands-and-dball)) |

| C5d 2.2 `dxStepIsland` in f64 | done: `csp_step::step_island` bit-identical to CSP's on 1,200 recorded car islands (bodies, joint feedback, solver inputs) ([below](#task-c5d-part-2-dxstepisland-in-double)). C5d done |

| C5e 1. CSP's world step decoded | done: `+0x92e4a0`, workers 0 and > 0 paths, auto-disable, island builder ([below](#task-c5e-csps-world-step)) |
| C5e 2. `csp_world::CspWorld::step` in f64 | done: 600-step Spa worlds of 1 and 3 cars of both proof cars bit-identical to CSP's, recorded at workers 0 and 7, the two recordings the same ([below](#task-c5e-csps-world-step)). C5e done |

| C5f 1. collision decoded, colliders on Spa | done: one pair, box against trimesh; `csp_btl` / `csp_opcode` / `csp_space` bit-identical to CSP's ([below](#task-c5f-csps-collision)) |
| C5f 2. reduction, contact joints, switches | done: `csp_reduce`, `csp_collide` bit-identical to CSP's; "soft deep" is the one step switch on |
| C5f 3. collision step, world on its own contacts | done: `csp_collision`; 600-step Spa worlds of 1 and 3 cars of both proof cars identical on the port's contacts. C5f done |

| C5g 1. wrapper calls recorded | done: `csp_oracle` kind `p`/`o` breaks log every outermost call into a `cphys::` wrapper and its return (records 7/8, `CSP_ORACLE_BREAK_SETUP=1` also logs the set-up); the switches decoded ([below](#task-c5g-csps-car-wrappers)) |
| C5g 2. port fed live, set-up compared | done (bodies, joints): `rustyac_ode::csp_core` fed every outermost call in process (`CSP_ORACLE_PORT=1`, `tools/csp_oracle/src/port.rs`); F2004: 6 bodies, 20 DBall + 1 fixed joint and their nodes identical at both step-0 world step entries, 85 set-up getter answers bit-exact. Getters after a world step are wrong until the port steps |
| C5g 3. car geoms, placement, the step | done: `csp_core` makes the car's boxes and mesh collider, places them with the car step's "Prepare spaces" (`+0x1688ab0` on the car container) and steps the world with its own collision; 600 Spa steps of F2004 and RSS3 with nothing copied but the track's and the car mesh's built trimesh data: 1200 world step entries each, bodies, joints, nodes, car spaces, geoms, boxes, OPCODE caches, kept pairs identical, 95655 / 83092 getter answers bit-exact. Part 1 done ([below](#part-1-proof)) |

| C5g 4. car against car | done: the car container's self-collide on the prepare's table, `dSpaceCollide2` of car spaces, `+0x92bab0`, the load reduction `+0x169fd90`, CSP's trimesh-trimesh collider (`csp_ttl`), two-body contact joints, kept car pairs; two F2004 touching on Spa, 1600 steps, 3200 world step entries identical with nothing copied, contacts word for word ([below](#part-2-car-against-car)). Proof area `csp` cases `core <car> <n>`. Part 2 done |

| C5g 5. Vanilla checks | done: `--quick` all areas identical (physics 9/9, fresh 63/63) except C5f's 4 `collision` cases, whose harness still marked car geoms dirty in the dynamic step and built no container table; the harness now takes CSP's flags after the prepare and builds the table, 4/4 identical |

| C5h 1. Embree linked, CSP's raycaster | done: `rustyac-ode` feature `embree` builds Embree 3.13.4 as CSP did (`build.rs`); `csp_ray` = `+0x932900`'s Embree half; every Embree ray of both proof cars identical to CSP's, at SSE2, AVX and AVX2 ([below](#task-c5h-the-tyre-rays)) |
| C5h 2. ODE's ray | done: `csp_raycast` = `+0x926270` in f64; 19,387 rays per car identical to CSP's with `ODE_BATCH_RAYCAST=1`, and every ODE ray of the default mode; builds for wasm |
| C5h 3. the whole world with own rays | done: `csp_core` casts the set-up's and the tyres' rays itself (Embree scene from the track's meshes, ODE ray on its own track space); 600 Spa steps of both cars and 1,600 of two F2004 touching identical with nothing taken from CSP |
| C5h 4. Vanilla | see [below](#vanilla-c5h) |

| C5i sizing | stopped before part 1 step 1 (rule 32): part 9 is 4-7 sessions, not 1-2 ([below](#task-c5i-sizing-the-whole-car)) |
| C5i-a 1. CSP's set-up from rustyAC's loaders | done: track, car boxes and mesh collider (`CLAMP_MESH_COLLIDERS`) 0 differ for both proof cars; the `core` cases run on them; `csp_ttl` b-major fix ([below](#c5i-a-part-1-csps-set-up-from-rustyacs-loaders)) |
| C5i-a 2. `PhysicsCore`'s second backend | done: `rustyac --csp` steps `csp_core`; smoke case `csp/smoke`; Vanilla bits and step time unchanged ([below](#c5i-a-part-2-physicscores-second-backend)) |
| C5i-a 3. Embree in `rustyac.exe`, CI, release | done: x64 Windows builds link Embree; uncached release build 24 min 52 s ([below](#c5i-a-part-3-embree-in-rustyacexe-ci-and-the-release)) |
| C5i-b 1. CSP's own record | done: `states.bin` per step, repeatable; `--compare-states` checked on Vanilla (1071 of 1106, the rest view-only) ([below](#task-c5i-b-the-whole-car-against-csp)) |
| C5i-b 2. rows | done: three C5i-a wiring mistakes fixed (lone car, `no_finite`, fixed joint ERP); proof cases `csp/row …` DIFFERENT, first difference CSP's tyre step (b) |
| C5i-b 3. CI's Embree, shortcut lists | done: one Embree build per CI cache miss (`RUSTYAC_EMBREE_LIB`); strut cars and loose-object tracks listed |
| C5i part 3 | done, nothing to change: the `csp` area keeps no results (CSP and the port run live on one CPU), and the port's Embree has CSP's ISA set, so both take SSE2 on the farm VM (Xeon E5620: SSE2, SSE4.2; no AVX, AVX2, AVX-512, FMA3) and AVX2 on this PC (i9-9980HK: all but AVX-512) |
| C5i-c 0. the C runtime | done: Windows builds link it statically (Embree `/MT`, `+crt-static`); no VC++ redist DLL imported ([below](#task-c5i-c-the-c-runtime-csp-modes-refusals-the-car-collision-callback-two-cars)) |
| C5i-c 1. strut cars, loose objects | refused, not ported: such a session runs Vanilla's core with CSP's data and says so; case `csp/strut abarth500` |
| C5i-c 2. callback `+0x587cc0` | done: `csp_hit`, equal to CSP's at every call of every row (break kind `f`) |
| C5i-c 3. Spa's track config | answered: real CSP gives an extended car CSP's air (the port is right); the oracle skips CSP's car-load finisher, RSS 3 row pinned EXPECTED |
| C5i-c 4. two cars | done: `oracle_gap`, row `csp/row ks_ferrari_f2004 two`; car 1 differs from step 1 (also in Vanilla), DIFFERENT |

| C5j 0. two cars' second car | done: it stood 19 cm into the climbing road; `oracle_lift` lifts it 0.2 m, car 1 matches to step 171 ([below](#task-c5j-csps-tyre-step-for-stock-cars)) |
| C5j 1. survey | done: CSP's tyre step runs Vanilla's for stock cars (no tyre extension) but for `updateAngularSpeed`'s tail |
| C5j 2. port | started: that tail ported (`angular_speed_tail`), bit-equal in process (kinds `U`/`V`); F2004 lap row's first difference step 2 → 63 |
| C5j s2 1. body difference | done: the lap tape had no collide keys (row set-up); CSP's car step sets the sleep limit to 250 frames (ported); F2004 lap/wall rows differ only in `tyre_grain` from step 1417/1414, the two-car row at the cars' contact (step 620) |
| C5j s2 3. RSS 3 air | more than small: the finisher's record wakes record-dependent CSP code (air, dirt, splines) that needs session objects the oracle lacks; pin stays |

| C5j s3 1. `tyre_grain` | done: the port's step was right; its `--physics-states` record left grain/blister 0 (`view.rs`); F2004 lap/wall rows IDENTICAL past the view list |
| C5j s3 2. two-car contact | done: the row ran the game's car on csp_oracle's stand-in box collider; `CSP_ORACLE_LOADER=use` on the rows, two-car row IDENTICAL past the view list |

**Resume here:** C5j: RSS 3 air and dirt (plan step 3, 2-3 sessions)
([C5j session 3](#session-3-neither-difference-was-in-the-physics)).

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

**Does a hit depend on the CPU?** Yes, CSP's own does (C5h): pinned to SSE2 or AVX the F2004's
600-step drive hashes `1a094e29d506e081`, at AVX2 (this PC's default) `c26ebd88f4c50b6e`. The port
matches CSP at each ISA when both are pinned to it (`CSP_ORACLE_ISA`), so the farm VM (SSE2 only)
needs its own kept results for the `csp` rows, or every side pinned to one ISA.

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
| 4 | the `66615b8a` code 0.13.1 does not have (C5d): `step.cpp` changes and the new LDLT / LCP solvers (CSP steps cars with `dWorldStep`, [answer](#does-csp-call-dworldstep-yes-every-physics-step)), contact and dball `getInfo2`, `matrix` changes (**done in C5d**, 1 session); the box / trimesh colliders that changed (1) and libccd pairs (1) moved to part 6. QuickStep (3,600 lines) only runs for islands whose bodies all carry CSP's flag `0x200`, never seen for cars: later, not counted | 1 used (planned 3.5-5.5) | per-stage in-process compare |
| 5 | CSP's world step: island builder, island jobs, worker count rule, post-step flags (**done in C5e**, 1 session; the space clean-up and geom flags go with part 6) | 1 used (planned 1-2) | whole world identical per step, workers 0 and 7 |
| 6 | collision: near callback, contact reduction, contact creation, the switches above; the box / trimesh colliders that changed (**done in C5f**: Spa uses box against trimesh only; libccd, car against car and the car collision callback go to part 7) | 1 used (planned 4-5) | contact joints identical per step, world on its own contacts |
| 7 | `cphys::` wrappers stock cars use: core, rigid bodies, dynamic collider, joints of the suspensions, raycaster, dummy suspension; and what CSP's loading jobs (AI grids, AI payloads, C5b) hand to the car | 2.5-4 | body states identical after set-up and per step |
| 8 | tyre rays: Embree linked, the default for every car (2-3); ODE ray path for `ODE_BATCH_RAYCAST=1` and the browser (1) (**done in C5h**, 1 session) | 1 used (planned 3-4) | hit points identical |
| 9 | whole-car rows in proof area `csp`, `rustyac --csp` steps CSP's core (C5i sized it: C5i-a 2-2.5, C5i-b 1-1.5, C5i-c 1-3) | 4-7 (planned 1-2) | rows identical on this PC |
| | **Total left after C5i's sizing** | **4-7** (part 9; after C5h: 1-2) | |

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

Licence: `66615b8a`'s `LICENSE-BSD.TXT` and `LICENSE.TXT` are unchanged since 0.13.1 and the BSD text is the one in `crates/rustyac-ode/LICENSE-ODE`, so [LICENSING.md](../../LICENSING.md)'s ODE row (BSD-3-Clause) covers the f64 side as it is.

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

## Task C5d, part 1, step 1: solver kernels in double

CSP builds ODE master `66615b8a`'s dense kernels (not 0.13.1's), so `rustyac-ode` gets them as a new
f64-only module, `crates/rustyac-ode/src/csp_matrix.rs`; the f32 path keeps 0.13.1's `matrix.rs`.
dSolveLCP (`+0x16ffe10`) calls them on the pairs layout (`b` and `d` interleaved, stride 2) with
`nskip = n <= 1 ? n : (n + 3) & ~3`. Copies in CSP:

| Kernel | stride 1 | stride 2 |
|---|---|---|
| `factorMatrixAsLDLT` | `+0x1700e90` | `+0x1701f60` |
| `solveL1Straight` | `+0x1701090` | `+0x1702750` |
| `solveL1Transposed` | `+0x17017b0` | `+0x1702e80` |
| `calculateLargeVectorDot` (`dxDot` = stride 1) | `+0x16cb540` | `+0x1703b80` |
| `scaleAndFactorizeL1Stripe_2` | `+0x1702440` | `+0x1703870` |
| `solveL1Stripe_2` / `solveStripeL1_1` | `+0x16fe080` / `+0x16fe3d0` | (shared) |
| `dxLDLTAddTL` / `dxLDLTRemove` | `+0x168ded0` / `+0x168e5d0` | |

Moves-only helpers (bit-trivial): `+0x16fe530` (swap problem), `+0x1703630` and `+0x1702160` (row
swaps). All kernels are scalar SSE2 except `solveStripeL1_1`'s packed block. The sums, as the
machine code adds them (`a0..` the products in source order):

- every unrolled loop adds its first columns one by one, as the source does;
- the big block is reassociated: 4 columns `((z + a0) + a1) + (a2 + a3)` (factor stripes),
  8 columns `(((z + a0) + a1) + (a2 + a3)) + ((a4 + a5) + (a6 + a7))` (L1 and L1T solves);
- `solveStripeL1_1`'s 4-column block is packed: `z + ((a0 + a2) + (a1 + a3))`;
- dot products: blocks of four add `(m3 + m2) + (m1 + m0)`, the rest one by one;
- `solveL1Transposed`'s odd first row starts `z = a`, not `0 + a` (matters for -0.0);
- `dxLDLTAddTL`'s first step: `alpha1 = alpha2 = 1` folded away, `W21 * W21` computed as
  `(a0/2 - 1)^2 * 0x3fe0000000000001` (the rounded square of `1/sqrt 2`), `k2 = (gamma1 * W21) * W11 - W21`;
- `dxLDLTRemove` divides (`L / d`), it has no reciprocal approximation.

Proof: `csp_oracle --solver <breaks.txt>` (`tools/csp_oracle/src/solver.rs`) calls the ten kernels
in the DLL and the port on matrices of 1 to 40 rows (row skip as dSolveLCP's, every third one 4
wider), 1,500 per size and set: values from the Spa drive's `dxStepBody` calls (`B*B^T + nI`,
positive definite, the solves and the rank-2 update run on CSP's factors), the same from random
values in [-1, 1], edge values and random bits; `ldlt_remove` with A behind shuffled row pointers.
240,000 inputs per kernel, at MXCSR `0x1f80` and `0x9fc0`: **0 differ** for all ten. Planted bugs
(each the source order instead of CSP's) show: factor stripe-2 block one by one 3,379 differ per
stride, stripe-1 block as `sum4` 14,479, L1 8-block one by one 10,737 / 10,937, L1T 10,984 / 11,116,
dot in source order 16,260 / 16,096, `dxLDLTAddTL` with `W21 * W21` 35,987, `dxLDLTRemove` with
`L * (1/d)` 602. Proof area `csp`, case `ode <car>`, now runs `--solver` after `--ode` (20 more
lines, 30 s).

## Task C5d, part 1, step 2: `dSolveLCP` in double

`crates/rustyac-ode/src/csp_lcp.rs` is master's `lcp.cpp` (Dantzig solver, `dLCP` with row pointers,
the NUB and findex moves) in f64 on `csp_matrix`'s kernels. `dxSolveLCP` is +0x16ffe10 (the all
unbounded path inline: factor<2>, L1<2>, scale, L1T<2>); the rest is outlined at +0x16fffa0 with the
`dLCP` constructor +0x16fe5f0, `transfer_i_to_C` +0x16febb0, `transfer_i_from_N_to_C` +0x16fed30,
`transfer_i_from_C_to_N` +0x16ff260, `pN_plusequals_ANi` +0x16ff3c0, `pC_plusequals_s` +0x16ff690,
`solve1` +0x16ff830, `swapProblem` +0x16fe530, `swapRowsAndCols` +0x1702160 (row data) and
+0x1703630 (row pointers). CSP's differences from the source:

| what | CSP |
|------|-----|
| `dInfinity` | 1e300, not infinity (+0x170013c / +0x170014e): bounds of 1e300 count as unbounded, true infinities do not |
| `s = -w(i) / delta_w(i)` and the cmd 4 step | `(-1 / dw) * w` |
| the "zero denominator" nudge | `nextafter(A(i,i), 1e300)` (+0x1da8110) |
| `==` / `!=` tests | `ucomisd` + `jne`: NaN counts as equal; `<` / `<=` tests that branch on `jae` / `ja` are `!(a >= b)` / `!(a > b)`, so NaN takes the "less" side |
| `transfer_i_from_N_to_C` dot | vectorised (+0x16fefd0): columns 4k, 4k+1 into lanes a, 4k+2, 4k+3 into lanes b, `(a0 + b0) + (a1 + b1)`, then the last `nC % 4` in order |
| everything else | source order (the other `ell . Dell` dot is `dxDot`) |

Proof (`csp_oracle --solver`, two more lines per MXCSR): problems shaped like a step's,
`A = B B^T + diag(1e-5 |v|)` with B 1 to 2n columns (often rank deficient), n 1..40, rows mixed
unbounded (1e300, one in eight true infinity) / contact normal / friction on an earlier normal's x
(findex) / bounded, with and without findex, values from the Spa drive, unit randoms and edge values;
compared are x, lo / hi, A after the call and findex. 36,000 general + 12,000 all unbounded problems
per MXCSR (0x1f80 and 0x9fc0): 0 differ. Planted bugs, each caught: N-to-C dot in lane order 1,469 /
1,569 differ, cmd 5 / 6 bounds swapped 6,371 / 6,547, s <= 0 exit without zeroing x 439 / 470, `==` with
NaN unequal 13,099 / 13,083, cmd 4 not zeroing w 245 / 246, C-to-N swap without state 1,565 / 1,638,
`-w / dw` 8,053 / 8,032. Case `ode <car>` now wants 24 solver lines.

## Task C5d, part 1, step 3: contact rows and the fixed joint's other path

**Contact joint** (vtable 0x182383170: getInfo1 +0x16c79d0, getInfo2 +0x16c7b30; layout: the_m +0xa0,
`dContact` from +0xa8, mode +0xa8, mu +0xb0 .. slip2 +0x118, pos +0x120, normal +0x140, depth +0x160,
fdir1 +0x180; reverse = flag 2 at +0x30; world contactp max_vel / min_depth at world +0xf8 / +0x100).
Port: `crates/rustyac-ode/src/csp_contact.rs`. Differences from master's `contact.cpp`:

| where | CSP |
|---|---|
| getInfo1 | every coefficient tested `>= 0`, so NaN is zeroed like a negative one; `dInfinity` = 1e300 |
| depth | `maxsd(depth - min_depth, 0)`: NaN gives 0 |
| normal rhs | `min(erp * fps * depth, max_vel) + motionN` (minsd: NaN takes max_vel) |
| bounce, body 1 | `((ja1 av1 + ja0 av0) + ja2 av2) - ((n1 l1 + n0 l0) + n2 l2)` |
| bounce, body 0 | `motionN - (((n2 l2 + n0 l0) + n1 l1) + ((ja2 av2 + ja0 av0) + ja1 av1) + outgoing1)`; new rhs by maxsd |
| dPlaneSpace | the `a * k` component is `sqrt(a)`; `|n2| > SQRT1_2` by comisd, so NaN takes the n0/n1 branch |

CSP's cars build their contacts in +0x18091c8e0: one contact becomes two world-attached joints (one per
body), mode `|= 0xe2` (FDIR1, MOTION1, MOTION2, MOTION_N), soft_cfm / slip1 / slip2 halved when set,
BOUNCE dropped below 0.02 or when the approach speed is under bounce_vel.

Proof (`csp_oracle --ode`, line `ode contact`): getInfo1 + getInfo2 on CSP's side and the port's,
compared are m, nub, the five coefficients after getInfo1 and per row J1, J2, rhs, cfm, lo, hi,
findex. Inputs: 4,180 contacts from the drive's bodies (contact point, normal and fdir1 from a third
body) with set coefficients, then the edge / bits / unit sets six at a time; modes from all surface
bits, every other one with CSP's `| 0xe2`, every third reversed, every fourth without body 1. 1,207,780
contacts per MXCSR (0x1f80, 0x9fc0): 0 differ. Planted bugs: body-0 linear dot in source order 1,641
differ; `a * k` in dPlaneSpace plus `min` after `+ motionN` 434,280 / 435,880. Case `ode <car>` now
wants 24 ode lines.

**Fixed joint's other path.** dxJointFixed (vtable 0x182382928, ctor +0x16a04f0 sets +0xe0..+0xf0 and
+0x128..+0x138 to the world ERP, +0xf8 to the world CFM, +0x100..+0x120 to -1). getInfo2 +0x16a0fd0:
+0x110 == -1 → master's path (proven in C5c); else +0x118 == -1 → +0x16a0670; else → +0x16a0a60.
The fields are set by CSP's `cphys::ode_impl_shifting_joint_base` (RTTI
`.?AUode_impl_shifting_joint_base@cphys@@`): a box body joined to the car by two fixed joints
(created in +0x1809272e0, bodies +0x180926e70); its updater +0x180927570 (from +0x180935280 after a
QueryPerformanceCounter delay, and +0x18093af30) copies 12 values p0..p11: +0xe0..+0xf0 = p6..p8
(linear ERP per axis), +0x128..+0x138 = p9..p11 (angular ERP per axis); if p0 = p1 = p2 and
p3 = p4 = p5 it sets +0xf8 = p0 and +0x110 = p3 (or -1 when p3 = p0), the rest -1; otherwise
+0xf8..+0x108 = p0..p2 (linear CFM per axis), +0x110..+0x120 = p3..p5 (angular CFM per axis).

What the two paths compute (both write nothing without body 1): rows 0..2 linear, rows 3..5 angular
(master has them the other way). Angular rows (+0x169fed0 / +0x16a01a0): J1 = I, J2 = -I,
q = q0⁻¹ q1 (dQMultiply1), error = qrel⁻¹ q (sign flipped when its w < 0), each component times
its axis' angular ERP, back to world by R0, times 2·fps; cfm = +0x110 (one value) or +0x110..+0x120.
Linear rows: +0x16a0670 keeps world axes (J1 = I with the cross of R0·offset, J2 = -I), rhs = R0 ·
(per-axis ERP · R0ᵀ (p1 - p0 + R0·offset)) · fps, one cfm +0xf8; +0x16a0a60 uses body 0's axes (J1 =
R0 columns, J2 = -R0 columns, angular part = (R0·offset) × axis), rhs = (R0ᵀ (p1 - p0) + offset) · axis
ERP · fps, cfm +0xf8 / +0x100 / +0x108. Not ported: no config key found that makes a car create
this joint, and neither proof car does (part 2's island capture checks the joints it steps).

## Task C5d, part 2: the car islands and dball

**What CSP's cars step.** `dxStepIsland` is +0x16c24b0 (context: world +0, h +8, memarena +0x10,
bodies +0x20, joints +0x28, nb +0x30, nj +0x34, allowed threads +0x38; one thread → Stage0 bodies
+0x16c26d0, Stage0 joints +0x16c35d0, then a tail jump to Stage1 +0x16c38f0). `dWorldStep` +0x16c6d00
hands it to the island processor. csp_oracle's island recorder (`CSP_ORACLE_BREAK=16c24b0i,16c6d00w`,
`CSP_ORACLE_ISLANDS=<file>`, tools/csp_oracle/src/island.rs) copies every island raw at entry and
again when the world step returns. On the 600-step Spa drive (h = 0.003 as f32→f64):

| car | island | joints |
|---|---|---|
| ks_ferrari_f2004 | 6 bodies | 1 fixed (6 rows), 20 dball (1 row each), 0..4 contacts on 8 steps |
| rss_formula_rss_3_v6_2026 | 6 bodies | 1 fixed, 20 dball, no contacts |

Every body has flag 0x100 (gyroscopic, so Stage0's implicit gyroscopic torque runs), world gravity
(0, -9.806, 0) at world +0x40, ERP 0.3 at +0x60, CFM 0 at +0x68, contactp max_vel 3 / min_depth 0.001 at
+0xf8 / +0x100. No joint has a feedback struct (+0x68 is null), so the island proof compares bodies.
The fixed joints keep +0x110 = -1 (master's path).

**dball** (vtable 0x1823829e8: getInfo1 +0x16a2fc0 m = nub = 1, getInfo2 +0x16a2fd0; anchor1 +0xa0,
anchor2 +0xc0, erp +0xe0, cfm +0xe8, target distance +0xf0, CSP stores target - |q| at +0xf8). Port:
`csp_joints::dball_get_info2`. Differences from master's `dball.cpp`: body 1's anchor sums as
`(a2 R_k2 + a0 R_k0) + (a1 R_k1 + pos_k)`; the short-q test is `!(|q| >= 1e-12)` (NaN takes it); body
0's point velocity takes `R a1` where the source has `globalA1 - pos`; the rhs reuses `|q|` from before
the velocity fallback; J1's angular part is `r × q` and J2's `q × r` as plain products (no `0 * q0`
term). Proof (`ode dball`, same inputs as the ball / fixed rows, velocities from spare slots): 1,207,788
joints per MXCSR, 0 differ. Planted bug (body-1 anchor in source order plus `len < 1e-12`): 273,898 /
271,603 differ. Case `ode <car>` now wants 26 ode lines.

## Task C5d, part 2: `dxStepIsland` in double

**Port.** `crates/rustyac-ode/src/csp_step.rs`: `step_island(world, bodies, joints, h)` is master's
`dxStepIsland` for one thread (Stage0 bodies and joints, 2a rows, 2b `J invM` and `rhs_tmp`, 2c A and
rhs, 3 `csp_lcp::solve_lcp`, 4 constraint forces, joint feedback, velocities, `dxStepBody`) over a slice
of `Body<f64>` and `Joint { kind, b0, b1, feedback }` (ball, fixed on master's path, dball, contact).
Master's ordering of the joints (unbounded, mixed, LCP) is kept as written; the "nub" master hands the
solver is the count of unbounded joints, not rows (CSP's too). Every stage was read in the machine code
(two-lane SSA trace of the SSE code, re/scratch/taskC5d/ssa.py); the CSP orders that differ from the
source:

| stage | CSP's order | master |
|---|---|---|
| Stage0 gyroscopic torque (+0x16c2c5e..+0x16c3529) | `tacc_x = (T02 L2 + T01 L1) + (T00 L0 + tacc_x)`, y: `(T12 L2 + T10 L0) + (T11 L1 + tacc_y)`, z: `(T22 L2 + T20 L0) + (T21 L1 + tacc_z)` | `tacc += T L` |
| Stage2c `MultiplyAddJinvMxJToA` (+0x16c47c0) | `A = ((AY + AX) + (AZ + A)) + ((LZ + LX) + LY)` | `A += ((((LX + LY) + LZ) + AX) + AY) + AZ` |
| Stage2c `MultiplySubJxRhsTmpFromRHS` | `rhs -= ((AZ + AY) + AX) + ((LX + LZ) + LY)` | sequential sum |
| Stage4 `avel += invI acc` (+0x16c5757) | z row: `(acc_y I21 + avel_z) + (acc_x I20 + acc_z I22)`; x, y as source | `dMultiplyAdd0_331` |

Everything else (world inverse inertia, the closed-form 3×3 inverse, `J invM`, `rhs_tmp`, the
constraint-force sums, the linear velocity) is source order up to commutative swaps.

**Proof.** csp_oracle's recorder now also breaks at `dxSolveLCP`'s entry (`16ffe10l`: m, nub, A's lower
triangle, rhs / cfm pairs, lo / hi pairs, findex and the bodies' force accumulators) and, with
`CSP_ORACLE_FEEDBACK=1`, gives every joint a zeroed `dJointFeedback` (CSP's cars set none; the step
only writes it). `csp_oracle --islands <file>` steps every recorded island with `csp_step` from its
entry state and compares bit for bit: every body's pos, q, R, lvel, avel, facc, tacc after the step,
every joint's feedback, and the solver's inputs. On the 600-step Spa drive:

| car | islands | with contacts | joint feedbacks | differ | solver inputs differ |
|---|---|---|---|---|---|
| ks_ferrari_f2004 | 600 | 8 (24 / 25 joints) | 12,626 | 0 | 0 |
| rss_formula_rss_3_v6_2026 | 600 | 0 | 12,600 | 0 | 0 |

Each step starts from CSP's recorded state (not a chained 600-step run of the port). Planted bugs:
master's A sum plus the source-order z row: 167,048 / 162,873 differ (solver inputs 73,679 / 75,614);
the gyroscopic y row in source order plus a feedback nudge: 30,900 / 31,783 (solver inputs 1,274 /
1,349). Proof area `csp`, case `island <car>` (record about 12 s, check under 1 s).

Not done in part 2: the other fixed path (CSP's shifting joint, decoded in step 3, no car of the proof
uses it) and hinge (CSP has none) are not in `JointKind`; ray-vs-trimesh stays part 8. Wiring
`csp_step` into `World` (body / joint lists, islands, auto-disable) is the next part's (5).

## Task C5e: CSP's world step

**What CSP does each physics step** (`+0x92e4a0`, CSP's physics world's step; addresses are offsets in
CSP's `dwrite.dll`):

1. Leaves at once while the late-step byte is set (C4a's `0x2803210` and its twin).
2. Collision: the pending contact batches (world `+0x328c` / `+0x3280` / `+0x3288`, `+0x92c9f0`) or
   the space collide (`+0x92c2b0`), then `+0x91d670` and the callbacks `+0x92b140`, `+0x938b40`,
   `+0x1226fa0`. Part 6's.
3. The ODE step, by worker count (`dll+0x267b338`, the dword `--workers` sets):
   - **workers 0** (CSP mode's rule): `h` as float widened to double, then
     `dxReallocateWorldProcessContext` (`+0x16c7650`, estimator `+0x16c5850`) and `dxProcessIslands`
     (`+0x16c6d00`) with the stepper hard-coded to `dxStepIsland` (`+0x16c24b0`). QuickStep never runs.
   - **workers > 0** (and the multi-thread byte): `+0x168d1c0` calls the same `+0x16c7650` with an
     estimator chooser (`+0x168d180`) and writes one 0x14-byte record per island (first body, first
     joint, body count, joint count, a byte "every body has flag `0x200`"); the job pool (`+0x5864f0`,
     job `+0x92ebd0`) hands islands out by an atomic counter, each job steps its island on one thread
     with `dxStepIsland`, or QuickStep (`+0x16bd3f0`) when that byte is set. After the jobs CSP marks
     every island body's geoms moved (geom `+0xc |= 2`, parents through `dGeomMoved`) and calls the
     body's moved callback (`+0x2c0`): the workers never call them.
4. Both paths join at `+0x92ea7d`: timing, the world's `+0x3230` countdown (back to 6), and with a
   global byte set the space clean-up `+0x168ccc0`. Part 6's.

`+0x16c7650` calls `+0x16c6ab0` (master's `BuildIslandsAndEstimateStepperMemoryRequirements`), which
runs auto-disable (`+0x16c5e60`) first. Both are master `66615b8a` with these differences:

| | CSP | master's C |
|---|---|---|
| joint enabled for islands | `flags & 8 == 0 && (!(0 >= b0.invMass) \|\| (b1 && !(0 >= b1.invMass)))` (`comisd`: a NaN mass counts as movable) | `invMass > 0` |
| auto-disable average | `buffer[0] + buffer[1] + …` then `(1.0 / samples) * sum` | same, but `dIASSERT` checks (none in CSP) |
| idle test | `!(dot > threshold)`: a NaN speed counts as idle | same expression |

**Port.** `crates/rustyac-ode/src/csp_world.rs`: `CspWorld { params, bodies, joints, nodes }` in
ODE's list order (newest first), `step(h)` = auto-disable, the island build above (body tag = its
index in its island), each island through C5d's `csp_step::step_island` and back. A type of its own,
not `World<f64>`: `World` holds f32 joints, geoms and spaces, and making all of it generic is part 7's
size, not this part's. Joint tags after the step are not set the way master does (`i` / `-1`); the
next build resets them, nothing reads them in between.

Found on the way: `step_island`'s constraint-force sum started from `+0.0`; CSP starts from the first
row's product, so a joint whose rows are all `-0` gives `-0` (seen in 3-car contact feedback). Fixed in
`csp_step`; the `island` cases re-run identical.

**Proof.** csp_oracle breaks at `+0x16c7650` (`a`: the whole world before auto-disable: world block,
every body with its sample buffers and joint list, every joint with feedback) and at the join
`+0x92ea7d` (`z`: the whole world after), so the same recording works at workers 0 and 7. `csp_oracle
--world <file>` steps each recorded world with `CspWorld::step` and compares every body (pos, q, R,
lvel, avel, facc, tacc, flags, tag, auto-disable state) and every joint feedback bit for bit, and prints
a hash of the after-states. The contacts are CSP's recorded ones (this proves the step, not
collision). 600-step Spa drive, cars 20 m apart along the spawn heading:

| car | cars | islands | contacts | differ (workers 0 / 7) | after-hash workers 0 = 7 |
|---|---|---|---|---|---|
| ks_ferrari_f2004 | 1 | 600 | 26 | 0 / 0 | yes |
| ks_ferrari_f2004 | 3 | 1,800 | 1,140 | 0 / 0 | yes |
| rss_formula_rss_3_v6_2026 | 1 | 600 | 0 | 0 / 0 | yes |
| rss_formula_rss_3_v6_2026 | 3 | 1,800 | 252 | 0 / 0 | yes |

Proof area `csp`, case `world <car> <n>`.

**Workers 7 against 0: identical, and why.** Islands share no body and no joint, every island is
stepped whole by one thread with `dxStepIsland`, and the moved callbacks and geom flags run after all
jobs on the calling thread in island order; so the order the jobs finish in cannot reach a bit. The
one way the worker count could change a result is QuickStep: an island whose bodies all carry flag
`0x200` takes QuickStep with workers > 0 but `dxStepIsland` with workers 0. No car body has that flag
(C5c, again here); who sets it stays an open question.

Auto-disable is ported but not proven: every recorded car body carries only flag `0x100` (gyroscopic), never
`0x10` (auto-disable), so CSP's `+0x16c5e60` skips every car body and so does the port. A world with an
auto-disabled body (none of CSP's stock objects found so far) would prove it.

Not done in part 5: the space clean-up, the geom moved flags and the body moved callback (no geoms in
`csp_world` yet: part 6), the `+0x3230` countdown (what reads it is not known yet), QuickStep.

## Task C5f: CSP's collision

**What CSP does in the collision part of a step** (`+0x92c2b0`, the "v2" path chosen by the byte
`dll+0x2803218+5`, set on every run seen). `v = *(world+0x20)`: `v[0]` the track's hash space, `v[1]` a
hash space holding one simple space per car (6 boxes and, for the F2004, a trimesh), `v[2]` / `v[3]`
empty on Spa. The world's `+0x4c` flips every step:

- **static steps** (`+0x4c != 0`): the kept car pairs (`+0x60`, none on Spa) through the pair processor
  `+0x9324a0`, then `dSpaceCollide2(v[1], v[0])` (`+0x168a5b0`) with the static callback `+0x92bef0`.
  The container has fewer children than the track, so the track's `collide2` (`+0x1689bf0`) runs once
  per car space; each (car space, trimesh) candidate comes back to the callback, which sees a space and
  calls `dSpaceCollide2(car space, trimesh)`: the car space's `collide2` (`+0x1688800`) cleans its
  children, then tests each enabled child against the trimesh (`+0x16880b0`: same body, category /
  collide bits either way, AABB overlap, both `AABBTest`s) and calls the callback with (box, trimesh).
- **the callback on a geom pair**: category and collide bits both ways (AND), the geom with a body,
  skip a disabled body; box (class 1 or 10): `dCollide` with 16 contacts, then reduction
  `+0x169f060` with `{4, 1, 4, 0.95}`; other classes 32 contacts and `+0x169fd90` (not seen on Spa);
  no contacts: done; else the contact joints (`+0x92c9f0`) and the pair is kept (`world+0x50`).
- **dynamic steps**: each kept pair through the pair processor (tag 0: `dCollide` 16, reduction,
  joints, no tests), the list emptied, then the car-against-car walk `+0x91bd40` (container
  self-collide, callback `+0x92bcb0`, 32 contacts and `+0x169fd90`, pairs kept in `+0x60`).
- `dCollide` (`+0x1687280`): `flags & 0xffff == 0`, same geom or same body: 0; poses refreshed; the
  colliders table (`qword_1828342C0`, 40 classes, `{fn, reverse}`); a reversed pair is collided the other
  way round, then each contact's normal negated and its geoms and sides swapped.

**Pairs on Spa.** One: box (class 1, on a car body) against trimesh (class 8), 16 contacts. 600 steps,
3 F2004s: 4,671 `dCollide` calls (4,519 from the static callback, 152 from kept pairs), 297 with
contacts. No sphere, capsule, cylinder, plane, ray or libccd pair, no car-against-car contact (cars
20 m apart). libccd and the other colliders are not ported: nothing on Spa calls them.

**Box against trimesh** (`+0x16b1b90`, master's `dCollideBTL` with CSP's OPCODE build): the OBB query
`+0x16d1890` (class I / II / III node tests, class III only with `fullBoxBoxTest`, which is on),
master's `InitQuery` with temporal coherence (four cache slots per box, round robin `+0x16a8210`, fat
box 1.1), master's triangle clip and contact merge. Port: `csp_opcode.rs` (`obb_query`, `BoxTc`,
`aabb_test`), `csp_btl.rs` (`collide_btl`). Proof: `csp_oracle` break kind `c` runs CSP's collider on
the live box and on 24 changed copies a call (noise, big moves, edge values, random bits, six flags
values) under MXCSR `0x1f80` and `0x9fc0`, contacts and cache bits compared: F2004 1 car 73,696
compared, 3 cars 228,879, RSS 3 cars 37,730, 0 differ. On Spa's meshes `Model::build` gives CSP's tree
node for node. Hash space `collide2` (`csp_space.rs`, kind `h`): candidates in CSP's order.

**Contact reduction** (`+0x169f060`): normal groups (join at `dot >= 0.95`), per-group quota, deepest
first then farthest, the selection sort's mixed `>` / `!(>=)` (NaN rules from the machine code).
`csp_reduce.rs`. Kind `r`: live calls plus 16 made-up inputs a call (up to 300 contacts, more than 8
groups, NaN / denormal / random bits): F2004 3 cars 5,049 + 5,049, RSS 3 cars 1,955 + 1,955, 0 differ.

**Contact joints** (`+0x92c9f0`). For a car box against the track: mode `0x701c`, mu 0.1 (0.4 with the
grip bit of the body's user data), bounce 0, erp `0.7142857142857143`, cfm `0.0009523809523809524`; a
contact is dropped when its float normal dotted with the body's up axis is below 0.9; deep contacts
(`depth > 0.07f`) are softened (`+0x91c520`, CSP's "soft deep" switch, on by default). Joints go to the
head of the world's list. `csp_collide.rs` (`contact_joints`, `soften`). Kind `j`: F2004 3 cars 297
calls, 1,140 contacts and joints; RSS 3 cars 115 calls, 448 contacts, 252 joints (196 dropped by the
up test); every joint word 0 differ; `soften` on 24 made-up cases a call, 0 differ.

**Switches that act on Spa.** In the step: "soft deep" (on). Off on every run: surface mixing,
depth-from-speed, the deferred batches (`+0x328c`), batched joints (`+0x3279`), the skip counter
(`+0x48`); the port says "not ported" when one is on. `HASH_SPACE_*`, `ISOLATE_CARS`,
`WHEEL_CAR_CONTACTS` (0) are not on the Spa path; `TRIMESH_OBB_PREFILTER` / `TRIMESH_AABB_TEST` are the
trimesh globals the oracle reads. `ENABLE_MESH_COLLIDER_SOONER`, `CLAMP_MESH_COLLIDERS`,
`EXTEND_COLLIDER_BOXES` act when the car's colliders are made (part 7's): their result is in the boxes
the oracle copies (not decoded).

**Geom poses and boxes.** `+0x1686600` (offset pose from the body), `+0x1686440` (rotation product),
box AABB `+0x1699680`, simple space AABB `+0x1688460` (children's union from `±1e300`; a NaN low side
is taken, a NaN high side is not), each with the machine code's operand order (a NaN's sign follows the
first operand). At collision entry every geom is already clean (`flags & 7 == 0`): code before the step
places them (part 7's). `+0x168ccc0` is not a space clean-up: it empties the contact joint group.

**Port.** `crates/rustyac-ode/src/csp_collision.rs`: `Geom`, `Shape` (box with its OPCODE caches,
trimesh, simple space), `Collision` (geoms, the track's meshes, car spaces, track hash space, kept
pairs, step kind, switches), `refresh` (pose and box), `CspWorld::collide` (the step above for box
against trimesh), `CspWorld::add_contacts`, `joint_of`.

**Proof** (`csp_oracle` break kind `q` at `+0x92c2b0` and its return): the oracle copies CSP's geoms,
spaces, bodies and kept pairs into a `Collision`, the car boxes and car spaces marked dirty so the port
places them from the bodies, runs `collide`, then compares the joints CSP made (order and every word),
the kept pairs, every box's OPCODE cache and every geom's flags, box and pose; 8 made-up poses a step go
through CSP's space `computeAABB` and the port's `refresh`, both MXCSR settings. The world recording
gets the port's joints (record 6) and `--world` steps every world on them instead of CSP's. 600-step Spa
drive, workers 0, proof area `csp` case `collision <car> <n>`:

| car | cars | steps compared | joints | differ | made-up poses (each MXCSR) | world on its own contacts |
|---|---|---|---|---|---|---|
| ks_ferrari_f2004 | 1 | 600 | 26 | 0 | 4,800, 0 differ | 600 steps, 600 islands, 0 differ |
| ks_ferrari_f2004 | 3 | 600 | 1,140 | 0 | 4,800, 0 differ | 600 steps, 1,800 islands, 0 differ |
| rss_formula_rss_3_v6_2026 | 1 | 600 | 0 | 0 | 4,800, 0 differ | 600 steps, 600 islands, 0 differ |
| rss_formula_rss_3_v6_2026 | 3 | 600 | 252 | 0 | 4,800, 0 differ | 600 steps, 1,800 islands, 0 differ |

Made-up poses with NaNs: 43-153 cases a run differ only as a NaN against a NaN of another sign or
payload (LLVM may swap a commutative operation's operands); counted apart, like the colliders'.

Planted bugs: clip grouping (1,517 + 1,471 differ), OBB dump order (8 + 12), reduction NaN rule and
`d2` grouping (2 + 2), `soften` grouping (139 + 136), up test on the X axis (32), pose grouping with
the reversed pair's side swap removed (189 steps, 727 poses): caught. Not caught: the OPCODE class II
grouping, the 0.95 up threshold and a row-for-column up axis (no Spa value near the edge).

**Not done in C5f.** Car against car (`+0x91bd40`, `+0x92bcb0`, `+0x169fd90`, pair tags 1 and 2): no
Spa drive has two cars touching; the port says "not ported" when two car spaces pass the pair test.
libccd and the other colliders (unused on Spa). A trimesh's own AABB (the track's are taken from CSP;
the car spaces' trimesh is never dirty). The joints' wake-up and body attach through the user data
(every Spa body is enabled and the geom's own). The car collision callback `+0x587cc0` (car side
effects, part 7). The geom placement before the step (part 7).

## Task C5g: CSP's car wrappers

Part 7 of the plan: the `cphys::` objects the car code makes through the core built by rustyAC.

### Switches decoded

Read once by `+0x1402bd0` from `extension/physics.ini`-level switches into bytes the wrappers test:

| Switch | Byte | Read by |
|---|---|---|
| `ENABLE_MESH_COLLIDER_SOONER` | `+0x280320a` | `+0x58dc00` (car collider status detour), `+0x92c9f0` (contact joints, at `+0x92d36f`) |
| `CLAMP_MESH_COLLIDERS` | `+0x280320b`, only read when SOONER is on (else 0) | `+0x58e720` |
| `EXTEND_COLLIDER_BOXES` | `+0x280320c` | `+0x413c20` (sets the box-size floor `+0x27f9950` to `max(INERTIA.z * 0.2, INERTIA.y * 0.5)` from car.ini `[BASIC] INERTIA` around the original call, then restores it), `+0x13ce600`, `+0x13d6840` |

Other wrapper bytes: `+0x280320f` (no finite rotation for new bodies: `+0x937f90`, `+0x936950`), `+0x2803208` (track meshes forced to arg 8 true: `+0x92f0f0`), `+0x28031fd` (`+0x1696450` on new trimeshes: `+0x92ff50`, `+0x921aa0`, `+0x926430`).

### The calls

`CSP_ORACLE_BREAK=<rva>p,...` on every wrapper function (the 166 of `tools/csp_oracle/wrappers.txt`, the wrapper vtables' functions, shared stubs left out) with `CSP_ORACLE_ISLANDS=<file>` records each outermost call (record 7: step, rva, thread, the four register and six stack arguments, xmm0..3, 8 words behind each pointer argument) and its return (record 8: rax, xmm0, 8 words behind rax and the call's rdx / r8). `CSP_ORACLE_BREAK_SETUP=1` logs the set-up too (step `usize::MAX - 1`).

On a 20-step F2004 Spa drive: set-up 6 `createRigidBody` (`+0x92ed80`), 6 `addBoxCollider` (`+0x933cc0`), 1 `addMeshCollider` (`+0x9337b0`), 20 `createDistanceJoint` (`+0x92f050`) with 20 `setERPCFM` (`+0x928310`) and 4 reseats (`+0x92fbb0` → `+0x928350`), 1 fixed joint (`+0x92fd20`), 4 raycasters (`+0x92fe80`), 455 track meshes (`+0x92f0f0`), 16986 set-up rays (`+0x92fac0`, Embree, part 8); per step about 300 calls: force / torque adders, getters, 4 ray casts, 20 `setERPCFM`, 2 reseats and the world step (`+0x92e4a0`, called twice, the one from `dwrite+0x67ee13` steps). The DUMMY / COSMIC suspensions (`+0x591f50` / `+0x592090`) are chosen by suspensions.ini `TYPE`; both proof cars are DWB on both axles, so neither runs.

### Part 1 proof

`csp_core` (in `crates/rustyac-ode/src/csp_core.rs`) now also holds the collision state:

- `addBoxCollider` (`+0x933cc0`): a box lower than the f32 floor `+0x27f9950` (0 unless EXTEND_COLLIDER_BOXES raised it at load) is made that high and its centre raised by half the difference; it goes in the car space of its key (`+0x91bdb0`, the +72 map: a simple space made in the car container the first time, its data the key), gets the body, an offset position, then `dGeomSetRotation` (`+0x1687da0`) with the body's own rotation, which sets the body's rotation and position again through `getBodyPosr` (`+0x1687900`).
- `addMeshCollider` (`+0x9337b0`, the trimesh path; the convex path behind byte `+0x280320d` and the remove path are asserted off): offset rotation the 4x4's 3x3 transposed, offset position the mesh centre (0) through the 4x4 in f32, then into the car space when wrapper byte +154 is set and the key is not -569524499 (`+0x939880`).
- `+0x933710` sets / clears wrapper byte +159 bits (bit 0: grip 0.4 in the joint maker), `+0x933750` sets mesh collider `i`'s collide bits.
- `dGeomMoved` (`+0x16881b0`), `dSpaceAdd`, `dGeomSetBody` keep ODE's list orders: a moved geom goes to its space's head, the space to the container's head.
- Placement: the car step (`+0x67e2e0`) calls `+0x1688ab0` on the car container before the cars step: `cleanGeoms` (`+0x1689360`) refreshes each dirty car space's dirty children (`+0x16886d0`, stopping at the first clean one) and the space. Nothing else places a geom; the world step moves each stepped body's geoms.
- The world step: collision (`csp_collision`) with the static and dynamic steps alternating, the ODE step, the contact joints dropped.
- The trimesh's own box (`+0x1694130`) is ported in `csp_collision` (`Shape::Mesh` carries the data's centre and half sides).

Proof (`PORT=1 SETUP_LOG=1 BRK=<fns.txt and 1688ab0, kind p> bash re/scratch/taskC5g/brk.sh <car> 600`): `port: 1200 world steps compared, 0 differ` for `ks_ferrari_f2004` (95655 answers) and `rss_formula_rss_3_v6_2026` (83092 answers, one box). Taken from CSP: the track's trimeshes and hash table at the first world step (C5f's reader), and the car mesh's built OPCODE data (tree, box, TC flag) at the mesh collider's return; building OPCODE trees is not ported.

Not reached by the proof cars: the dynamic collider (`+0x92ff50`), the split-along body (`+0x936950`, core +0x3298), DUMMY / COSMIC suspensions, a body offset (wrapper byte +156), reseat by point velocity. Rays (`+0x92fe80`, `+0x92b970`, `+0x92fb30`, `+0x92fac0`) are part 8. The collision callback `+0x587cc0` changes car state only (damage, events), part 9.

### Part 2: car against car

Decoded (dwrite IDA; every op order read from the machine code):

- **The dynamic step's car pass** `+0x91bd40`: the car container (a hash space) self-collides (`+0x16894f0`) with `+0x92bcb0` as callback. The self-collide rebuilds the table (`+0x1688ab0`) only when it was never built or with byte +218 and +217 set; otherwise it uses the table the car step's "Prepare spaces" built, from boxes one world step old (two world steps per car step). Pairs come out as in ODE's hash space: each entry against the cells of every level at or above its own, the first common cell only, same level only once; then each entry against each big geom, then big against big.
- `+0x92bcb0` / `+0x92bab0`: two car spaces go through `dSpaceCollide2` (`+0x168a5b0`: the space with more geoms, the first on a tie, is queried with each of the other's children through its `collide2`, `+0x1688800`, which cleans it first). Two geoms: categories both ways, one body awake, `dCollide` up to 32 contacts, the load reduction `+0x169fd90` with weight `max(p(g1), p(g2))` (`p`: wrapper byte +159 bit 2 → 1.0, bit 4 → 0.3f, else 0) and "at least three" unless the bodies' relative speed squared is over 1 (`+0x92ba10`), the contact maker `+0x92c9f0`, and the pair kept in the car list (+0x60) for the next static step, which runs it again through `+0x9324a0` (tag 0) before the track.
- **The load** (+0x31f8 of the core: counter, lo 50, hi 150, average, up 1.0, down 0.05): at each collision step's entry `avg = (counter - avg) * rate + avg` (down when `counter <= avg` or NaN), counter 0; `+0x169fd90` picks `per group / groups / total` from `strict = (1 - free) * (1 - weight)` and adds what the reduction kept to the counter.
- **Trimesh against trimesh** `+0x16bc1b0` (CSP's own, double): OBB prefilter (`TRIMESH_OBB_PREFILTER`, the smaller-volume mesh's box against the other's tree, `+0x16bbea0`), then CSP's OPCODE AABB tree collider (f32, `+0x16dfb50` / `+0x16e5900`: its own box-box sums, class III only at the root, the 9 edge axes of tri-box only with byte `+0x27f91a6`), each pair's triangles clipped against each other (`+0x16bce00`: deepest face, ±1e300 starts, 1e-8 / 1e-6 limits) and merged into a 256-bucket hash of contacts (`+0x16bbbc0`: key from `floor(p * 10000)`, same point within 1.732e-4, deeper replaces, equal depth averages the normals). The hint path (one body only) is not on the car-car path.
- **The contact maker with two bodies**: default surface (mode 0x701c, mu 0.25, bounce 0.01, erp 0.3, cfm 1e-4, softened past 0.1 m with "soft deep"), joints attached to both bodies. The callback `+0x587cc0` runs per side after it (car state, part 9).

Port: `crates/rustyac-ode/src/csp_ttl.rs` (new: the trimesh collider and CSP's tree walk), `csp_space.rs` `HashSpace::self_pairs`, `csp_collision.rs` (`Load`, the dynamic car pass, kept car pairs, trimesh pairs to `collide_ttl`), `csp_collide.rs` (two bodies, `Side::weight`), `csp_core.rs` (the container's table built at "Prepare spaces" when a car moved; `Core::step` returns its contact joints).

Proof: proof area `csp` case `core <car> <n>` (`tools/proofs/csp.sh`): the port fed every outermost wrapper call (`tools/csp_oracle/wrappers.txt`), nothing copied but the track and car mesh OPCODE data, compared at every world step entry (bodies, joints, nodes, car spaces, geoms, OPCODE caches, kept track and car pairs, the load) and every answer; the world recorded with the port's contact joints (record 6), `--world` checks them word for word against CSP's (bodies, contact words but the 4th words of pos / normal, which CSP's trimesh collider sets to 1.0 / the normal's length and nothing reads) and steps every world on them. The touch drive: two F2004 on Spa, car 0 parked (`CSP_ORACLE_PARK=1`, brake on), car 1 5.5 m behind (`CSP_ORACLE_GAP=5.5`) drives into it; 1600 steps, car-car contacts from step ~800. Break kind `t` (`+0x16bc1b0`) also calls CSP's trimesh collider on the live pair and on 8 moved shadows of the second geom (same body, own pose) against `collide_ttl`.

| case | world steps compared | differ | answers (set-up / steps) | trimesh pairs (live / shadows) | world on the port's contacts |
|---|---|---|---|---|---|
| core ks_ferrari_f2004 1 | 1,200 | 0 | 85 / 95,655, 0 wrong | none | 600 steps, 26 contacts, 0 differ |
| core rss_formula_rss_3_v6_2026 1 | 1,200 | 0 | 85 / 83,092, 0 wrong | none | 600 steps, 0 contacts, 0 differ |
| core ks_ferrari_f2004 2 | 3,200 | 0 | 169 / 500,791, 0 wrong | 102 / 816, 0 differ | 1,600 steps, 898 contacts, 0 differ |

Found on the way: the port had never read the joint maker's switches ("soft deep" stayed off; no single-car contact was deeper than 0.07 m, the touch's are 0.25 m).

Not done in part 2: pair tags 1 and 2 of `+0x9324a0` (never pushed on the drive), batched car-car joints (`car_pairs` switch, body flag 0x200), the moving-surface joint (wrapper +152 and not +153), a container rebuilt at every collide (+218, asserted clear), libccd, the box-trimesh path between cars (the boxes collide only with the track's category).

## Task C5h: the tyre rays

### The raycaster (dwrite IDA)

- `+0x92fe80(core, length)`: a tyre raycaster (32 bytes, `cphys::ode_impl_raycaster`): +24 the length,
  +16 an ODE ray geom (`+0x169dbb0`, class 5) with flags |= 0x10000 (first contact) | 0x20000 (back-face
  cull). Four per car, made in the set-up.
- `+0x92b970(raycaster, out, org, dir)`: the tyre's ray, `+0x92fac0(_, out, org, dir, length)`: the set-up's
  (16,986 on Spa for either car); both call the hook `+0x2817688` when set and `dir.y == -1` (never set),
  else `+0x932900(out, org, dir, length, ray geom or null)`.
- `+0x932900`: Embree when `+0x2820270` is null, byte `+0x28031ff` (Embree on: `ODE_BATCH_RAYCAST` 0) is 1 and
  `+0x9229f0`'s state byte (`+0x2838668`) is > 0. `rtcIntersect1` (tnear 0, tfar = length, mask and id left
  as they were, context flags 0, instID -1); miss: hit byte +32 and mesh +24 cleared; a hit is taken unless
  the ray is "down" (`dir.y == -1`, a ray geom, byte `+0x2812c7f` clear) and the face's `Ng.y < 0`: point =
  org + dir * tfar per axis, normal = Ng * (1 / sqrt((x² + y²) + z²)), flipped by `+0x922b80` when the mesh
  is two-sided (+40, all 455 Spa meshes) and faces away, mesh = list entry of `geomID`. Otherwise ODE's
  ray `+0x926270` (the thread's own ray `+0x924ff0`, flags |= 0x30000, when the caller gave none; its
  length was the cast's in every proof).
- `+0x9229f0`: the first call sets the state byte to -1 and posts the scene build (`+0x922b70` →
  `+0x9223e0`) to a pool; the state becomes 1 when it is done. **Which rays find it ready is a race**:
  4 runs switched after 16,986, 11,806, 9,431 and 9,319 of the 16,986 set-up rays; never later, so
  every tyre ray of every run went through Embree.
- `+0x926270(_, ray, out, org, dir)`: out = point 0, normal (0, 1, 0), mesh 0, hit 0; `dGeomRaySet`
  (`+0x169dc30`, origin and normalised direction in double); `dSpaceCollide2` (`+0x168a5b0`) of the ray
  against the track space (`**(+0x2817690 + 16)`) under its SRW lock with `+0x932f70`: spaces recursed,
  else both bodies null → `dCollide` with one contact, kept when its depth < the best (strict). Hit: point
  and normal to f32, mesh = the other geom's user data. The ray-first collider is the swapped
  `dCollideRTL` (`+0x16b2490`): ray flags at +12, max distance (f32) the ray length, the mesh's
  rotation as floats with no translation, origin (f32)(ray pos - mesh pos), OPCODE's `InitQuery`
  (`+0x16f4a80`) and segment stab (`+0x16f65c0`), groupings as the Vanilla port's; per face the
  world triangle in double (`+0x1694600`), its normal by the cross product, `dSafeNormalize3`, depth =
  the face distance, pos = depth * dir + ray pos.
- The mesh list `+0x2885370`: 455 mesh objects on Spa, newest first against the track space's geoms
  (list i = geom 454 - i), each with its ODE geom (+16), two-sided flag (+40), vertex / index block
  (+48: f32 vertices, u16 indices, the same arrays the trimesh got).

### Embree's build (part 1)

`crates/rustyac-ode/build.rs`, only with the feature `embree` (on in `tools/csp_oracle` only; the
browser and `rustyac` without `--csp` never link it): the source from `EMBREE_DIR`, else
`git archive v3.13.4` of `ignored/embree`; CMake with Ninja inside `vcvars64` (found by `vswhere`;
Visual Studio ships both), Release, `EMBREE_STATIC_LIB`, tasking INTERNAL, ISPC, tutorials and
filter functions off, ISA SSE2 + AVX + AVX2 on, SSE4.2 and AVX-512 off (CSP's strings: no ray masks,
back-face culling or filter functions, every geometry type), `CMAKE_POLICY_VERSION_MINIMUM=3.5` for
CMake 4. Needs on the PC: Visual Studio with the C++ tools (found by `vswhere -latest`), git (for the archive);
about 23 minutes at 4 jobs, 390 MB of build output per target folder. CI: a step clones
v3.13.4 into `$RUNNER_TEMP/embree` and sets `EMBREE_DIR`; the job's timeout is 90 minutes for an
uncached build. Built with MSVC 19.51; CSP used 19.29 (VS 2019 16.11): identical on every ray, so
the compiler does not matter here. Licence: Apache-2.0 rows in `LICENSING.md` and
`THIRD_PARTY_NOTICES.txt` (`tools/notices.py`).

Port: `crates/rustyac-ode/src/csp_ray.rs`: `Scene::new` (device string, quality HIGH, robust from
CSP's byte, one triangle geometry per list mesh with the list index as id, u16 indices widened),
`intersect` and `cast` (`+0x932900`'s Embree half: `Cast::Miss` / `Hit` / `Ode`).

### Proofs (parts 1 and 2)

`csp_oracle` break kinds `e` (`rtcIntersect1` `+0x18d40f0`, inside a cast only), `y` (`+0x932900`), `n`
(`rtcNewDevice` `+0x18d4260`: CSP's device string replaced by the oracle's, `CSP_ORACLE_ISA=<isa>` adds
`max_isa=` on both sides), `v` (`+0x926270`, the port's ODE ray against a copy of CSP's track space;
return sites `s`: `x`, `k` and `u` are the island and collision recorders'),
`tools/csp_oracle/src/rays.rs`. `SETUP_LOG=1 BRK=18d40f0e,932900y,18d4260n,926270v bash
re/scratch/taskC5h/brk.sh <car> 600` (`ODE=1` writes `[PHYSICS_EXPERIMENTS] ODE_BATCH_RAYCAST=1`):

| run | Embree intersections / casts (wrong) | ODE casts (wrong) | drive hash |
|---|---|---|---|
| F2004, default | 9,955 / 9,955 (0) | 9,432 (0) | `c26ebd88f4c50b6e` |
| RSS3, default | 9,447 / 9,447 (0) | 9,940 (0) | `9dfc977c30fa86af` |
| F2004, `ODE_BATCH_RAYCAST=1` | none | 19,387 (0) | `410bb67f9225aab0` |
| RSS3, `ODE_BATCH_RAYCAST=1` | none | 19,387 (0) | |
| F2004 pinned to SSE2 / AVX / AVX2 | 2,400 / 2,400 (0) each | | `1a094e29…` / `1a094e29…` / `c26ebd88…` |

Every compare is bits: Embree's tfar, geomID and on a hit Ng, u, v, primID; the out struct's point,
normal, mesh and hit byte. The ODE rays' flags are only ever 0x3001d / 0x30019 (first contact and
cull, no closest hit). wasm: `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="--sysroot <target/wasm-sysroot,
Windows form>" cargo clippy -p rustyac-ode --target wasm32-unknown-unknown -- -D warnings` clean.

Port of the ODE ray: `crates/rustyac-ode/src/csp_raycast.rs` (`cast(&Collision, org, dir, &Ray)`), the
OPCODE half shared with Vanilla's `collide_ray_mesh` through `collision::stab` (Vanilla unchanged).
Unhandled: closest-hit rays, trimesh ray callbacks, quantised OPCODE trees (one Spa mesh has one; no
ray reached it).

### Part 3: the whole world with own rays

`csp_core::Core::set_rays` (the list's meshes, Embree's settings, `+0x2812c7f`) and `Core::cast`
(`+0x932900`: Embree once ready, else ODE's ray on the port's own track space, cleaned as
`dSpaceCollide2` cleans it: stale boxes made, dirty marks cleared, table rebuilt). The port's track
now comes in at the first ray (`port.rs` `track_in`; the core is `+0x2817690` less 16), not at the
first world step, since 16,986 rays come first. **The race**: the port's model is "ODE until the first
tyre ray, Embree from it", what every run above did; the oracle pins CSP to it (`rays.rs` `pin`: the
state byte held at -1 until then, then CSP's build started and waited for). Drive hashes with the pin
equal the unpinned ones above.

Proof (`tools/proofs/csp.sh` `core <car> <n>`, breaks `932900y,18d4260n` added; run by
`re/scratch/taskC5h/p3.sh`), nothing taken from CSP but the track's built data:

| case | world steps compared | differ | answers set-up / steps, wrong | rays set-up / tyre | trimesh pairs |
|---|---|---|---|---|---|
| core ks_ferrari_f2004 1 | 1,200 | 0 | 17,071 / 98,055, 0 | 16,986 / 2,400 | |
| core rss_formula_rss_3_v6_2026 1 | 1,200 | 0 | 17,071 / 85,492, 0 | 16,986 / 2,400 | |
| core ks_ferrari_f2004 2 | 3,200 | 0 | 17,155 / 513,591, 0 | 16,986 / 12,800 | 102 / 816, 0 differ |
| F2004 with `ODE_BATCH_RAYCAST=1` | 1,200 | 0 | 17,071 / 98,055, 0 | all ODE | |

Traps: Embree's first device made inside a break hung (`rtcNewDevice` never returned; on its own
thread too): the oracle makes an empty scene at start when the port is on. Before the first world
step the track's boxes are stale (flags 0x1d): ODE's ray cleans the space first, so the port must too.
The track space is `**(+0x2817690 + 16)`, not `*(+0x2817690 + 16)`.

### Vanilla (C5h)


Vanilla's ray against the trimesh (`collision.rs` `collide_ray_mesh`) now calls the OPCODE half it shares
with `csp_raycast` (`collision::stab`), unchanged otherwise; `rustyac-ode` and `rustyac-physics` unit tests
pass. `tools/proofs/run_all.sh --quick` (2026-11-20, 1 h 06 min): physics 9/9, fresh 63/63, render 18/18,
gui 29/29, browser 18/18, ai_page 10/10, net 16/16, replays 6/6, dumps 6/6, shaders 4/4, sound 4/4,
python 1/1, launcher 1/1 identical. The csp area's first run found two oracle faults of this task (both
fixed): the ray break kinds had taken the island and collision recorders' letters (`x`,
`k`, `u`; the ray kinds are now `v` and `s`), and a script edit had turned `csp.sh`'s literal CR in
`tr -d` into a newline (every `words` case). Rerun of the csp area: 64/64 identical (13 min), the three `core` cases on the port's own rays among them.

## Task C5i: sizing the whole car

C5i (part 9) was planned at 1-2 sessions. It stopped before part 1 step 1 (rule 32), because the step
is several sessions on its own:

- **Nothing in rustyAC steps `csp_core` yet.** Only `tools/csp_oracle` builds a `Core`, and it feeds
  it from CSP's wrapper calls (`port.rs`) and from CSP's memory (`collision.rs` `port_track`: geoms,
  hash levels, `static_step`, `aabb_limit`, the prefilter / edge bytes; `rays::ray_meshes`: the Embree
  mesh list). `rustyac --csp` (`crates/rustyac-physics/src/csp.rs`) only changes how content is read.
- **`PhysicsCore` needs a second backend.** `car/body.rs` wraps Vanilla's f32 `World` directly
  (about 70 methods), and 65 more places in `rustyac-physics` reach `.world.` or `JointId`
  (`car/replay.rs` 19, `engine/mod.rs` 12, `race/referee.rs` 11, the suspensions, the track loader).
- **The set-up must come from rustyAC's own loaders.** The pieces are proven (`Model::build` gives
  CSP's tree node for node, `HashSpace::build`, the box floor of `EXTEND_COLLIDER_BOXES`, the mesh
  list's newest-first order), but nothing yet builds CSP's track (455 meshes on Spa) and car colliders
  from the track and car files instead of copying them from CSP.

Estimate, as three tasks:

| Task | Work | Sessions |
|---|---|---|
| C5i-a | `PhysicsCore` on `csp_core` with `--csp`; CSP's track and car set-up built from rustyAC's loaders and checked against `port_track`'s copy; Embree in the Windows desktop `rustyac.exe` (release workflow, CI, licence rows) | 2-2.5 |
| C5i-b | whole-car rows: `rustyac --csp` against `csp_oracle` per step (pose, velocities, tyres, suspension, engine, controls), F2004 and RSS 3 V6 on Spa, 600+ steps; first verdicts | 1-1.5 |
| C5i-c | the first differences: CSP car code outside the core that is not ported (unknown size until C5i-b's rows run), the car collision callback `+0x587cc0` if a row reaches it, then v1.1.5 | 1-3 |

**Part 3 (CPU answer), done.** Every `csp` case runs CSP and the port live on one machine and compares
in that run; the area keeps no results, so there is no cache key that Embree's ISA should be part of.
The ISA only has to go into a key when a C5i-b row keeps a recording. The port's Embree is built with
CSP's ISA set (`crates/rustyac-ode/build.rs`: SSE2, AVX, AVX2; SSE4.2 and AVX-512 off), so on any CPU
both pick the same kernels. Measured CPU flags (`IsProcessorFeaturePresent`, FMA3 from MSVCR120's
`_set_FMA3_enable`):

| Machine | CPU | SSE2 | SSE4.2 | AVX | AVX2 | AVX-512 | FMA3 | Embree picks |
|---|---|---|---|---|---|---|---|---|
| farm VM | Xeon E5620 | yes | yes | no | no | no | no | SSE2 |
| this PC | i9-9980HK | yes | yes | yes | yes | no | yes | AVX2 |

### C5i-a part 1: CSP's set-up from rustyAC's loaders

`crates/rustyac-physics/src/csp/core_track.rs` (`build(&Track) -> CspTrack`) and `core_car.rs`
(`build(data, game_root, car) -> CspCar`) build what `port_track`, `ray_meshes` and the car's
collider calls used to copy out of CSP. `csp_oracle` checks them with `CSP_ORACLE_LOADER=compare`
(compare only) or `=use` (compare, then step on the loaders' set-up instead of CSP's copy); the `core`
proof cases run with `use` and need the "loader track" / "loader car" lines at 0 differ.

What the loaders had to learn:
- **Track.** One-triangle meshes (Spa geom 391) keep no tree data (mesh `None`, `tree_test` false).
  The Embree list is pushed per mesh geom n-1-i, then reversed. Category 1, hash levels (2, 11),
  `aabb_limit` 8, full box-box on, car levels (0, 5), prefilter on, edges off (switch table above).
- **Car boxes.** The `EXTEND_COLLIDER_BOXES` floor is skipped (CSP +0x413850) when the data has
  `script.lua`, or `colliders.ini` has `[_EXTENSION]` or a `COLLIDER*` section with `MAX_WEAR`
  (RSS 3 V6: floor 0).
- **Car mesh (`CLAMP_MESH_COLLIDERS`, detour +0x58e720 of `Car::initColliderMesh`).** Each vertex
  plus `[_EXTENSION] MESH_OFFSET`, through the body matrix, y raised to the lowest box bottom
  (`min(CENTRE.y - SIZE.y/2)`, start 1.0 or suspensions.ini `[FRONT] BASEY`), back through CSP's own
  4x4 inverse (+0x304a40, cofactors, not DirectXMath's). Under 0.3 m between top and floor: warning,
  car flag 0x200, mesh unchanged. The f32 round trip moves most vertices by an ulp, so the clamp
  has to run even where no vertex is raised.

Proof (F2004 and RSS 3 V6 on Spa): geoms 455/455, meshes 454/454, ray meshes 455/455, settings,
boxes (F2004 6, RSS 1) and the mesh collider (bits, node for node) 0 differ. The C5h `core` cases on
the loaders' set-up: F2004 600 steps (1200 world steps, set-up 17071, steps 98055, 0 wrong), RSS 600
(85492 steps, 0 wrong), two F2004 1600 steps (3200 world steps, 512937 answers, ttl live 978 and
shadow 7824 calls, 18078 contacts, all 0 differ).

**`csp_ttl` traversal fix.** With the real collider.kn5 instead of `csp_oracle`'s 8-vertex stand-in
box, 60 of 7824 shadow calls had the same contacts in another order. `+0x16e5900` is not stock
OPCODE: when both of a's children are nodes and b has a leaf, b's slots are the outer loop (each b
leaf fetched once, walked down a.pos then a.neg; b node children `collide(a.pos, ·)`,
`collide(a.neg, ·)`). Every other case is a-major as before.

### C5i-a part 2: `PhysicsCore`'s second backend

**Shape.** `PhysicsCore` (crates/rustyac-physics/src/car/body.rs) keeps Vanilla's f32 `World` as it was
and gains `csp: Option<Box<CspBody>>` (crates/rustyac-physics/src/csp/core_body.rs). `None` is Vanilla:
every method runs its old code. `Some` is CSP mode: each method's first line hands the call to
`CspBody`, which drives `rustyac_ode::csp_core::Core` (the double world, CSP's wrappers, collision and
ray code proven in C5g/C5h). One check of an `Option` is what Vanilla pays. A body's `RigidBody` id
is the CSP body id, a joint's `JointId` the CSP joint id, a `GeomId` the index into CSP's geoms.
`PhysicsEngine::set_track` turns the backend on (`use_csp`) when CSP mode is on, before any body is made.

**One track core.** `Track::csp_core()` builds CSP's track once per loaded track (part 1's
`core_track::build` → `Core::set_track`, `set_rays`, the track space cleaned as the cars' first tyre ray
cleans it). Every physics core of CSP mode starts as a copy of it (the trimesh data is shared,
`Arc<TrackMesh>`). The tyres' rays go to a second copy behind a lock (`CspCore::tyre_ray`):
`Track::ray_cast` asks it in CSP mode, so cars, replays, the net session and the recorder get CSP's rays
through the same `RayTrackCollisionProvider` as before.

**The engine's copies.** The engine keeps one world and each car a copy (as in Vanilla). The direct
`.world` reads and writes in the engine, the state hash, the HUD, the AI's car view, the net replay rows,
the chassis trace and the recorder go through neutral calls instead: `body_state` (pos, q, velocities,
R, mass, inertia, force accumulators as f32), `slot_counts`, `copy_body_state_from`,
`copy_joint_state_from`, `copy_geom_bits_from`. `csp_core::Core` got the matching pieces: `step` split
into `collide` + `step_world`, `copy_body_from`, `copy_joint_from`, `geom_body`, `joints_made`, `clean_track`.
Left on Vanilla's world on purpose: `car/replay.rs`'s 18 sites (the oracle runner replaying the game's
own Vanilla calls), the recorder's fault injection, and the oracles in `tools/`.

**What CSP mode does not have yet (shortcuts, each marked in the code).**
- Slider and ball joints (only the strut suspension makes them): CSP mode asserts. No strut car is
  among the CSP cars tested.
- Loose track objects (cones, `--track-objects`): CSP mode builds none.
- `set_contacts` (the oracles' recorded contacts) and the force tape stay Vanilla-only.
- `no_finite` (CSP byte +0x280320f) is taken as false (its default). Wrong: it is `OPTIMIZE_ROTATION`,
  default 1; fixed in C5i-b ([below](#task-c5i-b-the-whole-car-against-csp)).
- Local-force calls CSP never recorded (`ADD_LOCAL_FORCE_AT_POS`, `ADD_LOCAL_TORQUE`, the
  velocity setters, `local_to_world_normal`) are made from world vectors with CSP's own maths.
- A build without Embree (the browser, non-x64 targets) casts ODE's rays (CSP's `ODE_BATCH_RAYCAST=1`
  path). Since part 3 the Windows `rustyac.exe` casts Embree's, as CSP does by default.

**Smoke test** (proof case `csp/smoke`). `rustyac --csp`: the F2004 (autodriver) and the RSS
(`--opponent …:auto`) together for 10 s at Spa, recorded, then replayed twice. Result: 3700 steps, no panic, the F2004
at 59.8 km/h on the road, both replays hash `120d2b99955bbd8b`. Also by hand: each car alone for 20 s
(after 20 s the F2004 is at 298.1 km/h in CSP mode and in Vanilla, 1 m apart: C5i-b found a lone car
still on Vanilla's world), two
F2004, and a F2004 against an AI F2004 (the autodriver runs into the AI car at La Source in both modes).
Replaying a CSP recording runs CSP mode (`csp=1` in the header). The same inputs with `csp=0` give
another hash, so the backend is really in use.

**Step time.** Vanilla, the same 60 s Spa drive (20463 steps, hash `bd3caf77f90d3989` before and after),
7 replays each, interleaved, while another program loaded the PC: before median 0.1441 ms a step,
after median 0.1440. (Earlier, with the PC idle, before was 0.0573.) CSP mode, one F2004, 3791 steps:
0.066 ms a step on ODE's rays, against 0.053 in Vanilla. Two cars: 0.35 ms (the car-against-car tree
collider). With Embree (part 3): 0.043 ms.

### C5i-a part 3: Embree in `rustyac.exe`, CI and the release

- **Which builds.** `crates/rustyac-game/Cargo.toml` turns on `rustyac-ode`'s `embree` for
  `cfg(all(windows, target_arch = "x86_64"))`: the release's `rustyac.exe` (and the launcher, which links
  rustyac-game). The browser (wasm) and the other `platforms.yml` targets keep ODE's rays: Windows on
  ARM cannot build Embree's x86 ISAs. Vanilla never calls it.
- **Build.** `crates/rustyac-ode/build.rs` builds Embree once per target folder (`target/embree-3.13.4-mt`,
  shared by the profiles, clippy and the tests; built in `OUT_DIR`, then moved). Source: `EMBREE_DIR`,
  else `ignored/embree` at tag v3.13.4. It stops with what is missing: no source (with the clone line),
  no Visual Studio with the C++ tools (vswhere), or CMake / Ninja failing.
- **CI.** `ci.yml`, `release.yml` and (x64 Windows only) `platforms.yml` clone v3.13.4 into
  `$RUNNER_TEMP/embree` before the build. CI caches `target` and `tools/*/target`, so the Embree build is
  cached with them (a cache miss: about 20 minutes, three times: the workspace's, `csp_oracle`'s and
  `web_pack`'s, which links rustyac-game through rustyac-web). The release builds the workspace
  without a cache: `timeout-minutes` 60 → 120.
- **Build time.** An uncached `cargo build --release --workspace --locked` on this PC (4 jobs): 24 min
  52 s, Embree about 20 of them (no other crate built while it ran). `rustyac.exe`: 12.1 → 30.1 MB.
- **Licence.** Embree is Apache-2.0: `LICENSE-APACHE` is the same text as Embree's `LICENSE.txt`, and
  `tools/notices.py` already writes its row (`THIRD_PARTY_NOTICES.txt`; Embree is not a crate, so the row
  is fixed in the script). The library's sources (`common`, `kernels`, `include`) carry only Intel's
  copyright; Kitware's lines are on CMake scripts and `sse2neon.h` (MIT) is ARM only. LICENSING.md
  says so.
- **Proofs.** The `csp/smoke` case on the Embree build: 3778 steps, the F2004 at 61.3 km/h, both replays
  `d20a8d396f2d1309`. Vanilla, the 60 s Spa drive with the PC idle, 5 runs each, interleaved: before
  median 0.0517 ms a step, after (Embree linked) 0.0506; hash `bd3caf77f90d3989` both. `run_all.sh
  --quick` (the `csp` area is all its cases): every area identical (physics 9, fresh 63, csp 65,
  render 18, gui 29, browser 18, ai_page 10, net 16, the rest too); workspace clippy clean.

**Resume here (C5i-a):** done. Next: C5i-b ([sizing](#task-c5i-sizing-the-whole-car)).

## Task C5i-b: the whole car against CSP

### Part 1: CSP's own record

`csp_oracle --drive` writes `car_root/states.bin`: after every physics step, every car's
`CarPhysicsState` as CSP's `Car::getPhysicsState` gives it (the game's 0xb70-byte layout, the one the
Vanilla physics area compares). `rustyac --physics-states <file>` writes the same layout from
`rustyac --replay --headless` (`CarView::capture_world`). `csp_oracle --compare-states <game> <port>
<cars>` compares them value by value (`member[k]`: line k of the member's `{:#?}` text; an f32 prints
so it reads back to its bits) and prints, per value, the first step it differs, earliest first.

- **Repeatable.** The F2004 on the Spa lap tape, 1000 steps, run twice: `states.bin` byte for byte the
  same (steps hash `fab5e99c7314a9b6` both).
- **The comparison itself, on Vanilla.** `csp_oracle --vanilla` (acs.exe's own car, CSP not loaded)
  against `rustyac --replay` of the same tape, F2004, 300 steps: 1071 of 1106 values equal at every step.
  The 35 others are fields rustyAC's view does not fill like the game's (sector ids of
  `tyre_surface_def`, `last_ff_final`, `body_work_volume`, `time_stamp`, `ride_height`, `setup_state`,
  `gear_rpm_window`, the view half of `tyre_thermal_states`, `lock_controls_time`, `anti_squat`,
  `caster`), all constant from step 0. Pose, velocities, every tyre value, suspension, engine and gear
  are bit-equal.
- **CSP's view.** CSP's `getPhysicsState` hook (`+0x590220`, first chunk 3076 bytes) rewrites some
  fields after the step: it low-passes `acc_g` (0.25) and the wheels' angular speed (the wall row's
  step-1 tyre speeds are exactly 0.25 × the port's), and fills `ground_plane` and
  `normalized_spline_position` from its own plane and spline. Those are view values; the proof case
  skips them when it names the first differing value.

### Part 2: the rows (proof area `csp`, cases `row <car> <drive>`)

Each case: CSP live drives the tape (`csp_oracle`, workers 0, `RANDOMIZE_WHEEL_ANGLES=0`), the same tape
(car swapped, `csp=1`, cut after the case's steps) replayed by `rustyac --csp` with the same start, the
two records compared; `CSP_ORACLE_BREAK=587cc0` counts CSP's car collision callback. Live on one PC, no
kept recording (so no Embree ISA key).

**Two wiring mistakes of C5i-a found and fixed (a).** A per-step core trace (`CSP_CORE_TRACE=<file>`:
every force, torque, body, joint and world step of `csp_core`, written by `rustyac --csp` and by
`csp_oracle`'s port fed CSP's own calls) found them:

1. **A lone car never used CSP's core.** Only `PhysicsEngine::set_track` (two or more cars) switched the
   core to CSP's; a car alone (and the replay runner's car) built Vanilla's f32 world with CSP's rays.
   C5i-a's "alone, CSP and Vanilla 1 m apart after 20 s" was that. `RayTrackCollisionProvider::track`
   now gives the track and `RollingChassis::new_with_objects` calls `use_csp` in CSP mode.
2. **Body flags.** CSP makes every car body with `no_finite` set: the byte `+0x280320f` is general.ini
   `[PHYSICS_EXPERIMENTS_DONE] OPTIMIZE_ROTATION` (hidden, default 1; written by `+0x1402bd0` at
   `+0x1403f04`), not false as C5i-a took it.
3. **The fuel tank's fixed joint.** rustyAC set its ERP to 0.9 below 1 m/s, as Vanilla does; CSP's fixed
   joint's `setERPCFM` (vtable `+0x23051f0` slot 1) is a bare `ret`, so it stays 0.3.

After them the first three world steps' entries (bodies, joints, parameters) are identical, and the
first difference is a tyre force at physics step 2.

| Row | Steps | Verdict | Values equal at every step | First differing value (past CSP's view) | `+0x587cc0` calls | Class |
|---|---|---|---|---|---|---|
| `row ks_ferrari_f2004 lap` | 4000 | DIFFERENT | 644 of 1106 | step 2 `world_matrix`: 2.0008296e-5 (CSP) vs 2.005232e-5; from front-left tyre's force at step 2 (load 259.70 vs 259.30) | 26 | (b) CSP's tyre step |
| `row rss_formula_rss_3_v6_2026 lap` | 4000 | DIFFERENT | 610 of 1106 | step 0 `air_density`: 1.1855999 (CSP, Vanilla's law) vs 1.1127756 (port, CSP's extended law) | 621 | open (oracle limit) |
| `row ks_ferrari_f2004 wall` | 6667 | DIFFERENT | 621 of 1106 | step 154 `world_matrix[3]`: -0.5623173 vs -0.56231904 (CSP's view fields differ from step 1) | 3977 (from step 55) | (b) as the lap row |
| two F2004 touching (C5g layout) | 1600 (CSP only) | not run on the port | | | 1599 (from step 65) | C5i-c |

- **F2004 (b).** Every core call before the tyres' first forces matches; the front-left tyre's force at
  step 2 has the same direction and 0.15 % more load in CSP. The force comes from `Tyre::step`, which
  CSP replaces for stock cars too (`docs/csp/physics_tyres_suspension.md` §1-2: gas-law pressure,
  vertical-stiffness conditioning, brake torque limit, CSP's contact search, `getCorrectedD`,
  `stepDirtyLevel`; plan item CSP-7). The exact item is not found yet. Three hidden switches set to 0 in
  the oracle's `general.ini` (`BRAKES_LIMIT`, `SLIDING_FIX`, `SMOOTH_CONTROLS_LOCKING`) left the row as it
  was (877 of 1106 at 50 steps each time).
- **RSS 3 (open).** The car is `extended-2`, so the port uses CSP's air law (C4a, proven against CSP's
  own function). CSP live in the oracle keeps Vanilla's density at every step, though its extended-car
  creator (`+0x5e32e0`) runs once and `+0x588850` / `+0x5bdfa0` every step: in the oracle CSP has no
  track config (C4a: a registered record makes CSP's whole extended step run, which needs it), so this row
  cannot tell which side is right until the oracle loads CSP's track config.
- **The callback.** `+0x587cc0` (first chunk 2946 bytes; car state only: damage, events) is reached by
  every row, the lap too (26 times in 4000 steps), not ported.
- **Two cars.** Not run on the port: the oracle-tape runner is a lone car, and the C5g layout (the second
  F2004 parked 5.5 m beside) has no spawn on the replay side. The callback is reached; the row goes to
  C5i-c with the rest, as the single car already differs at step 2.

**(b) list for C5i-c.**

| Item | Where | Size | Rows |
|---|---|---|---|
| CSP's `Tyre::step` for stock cars (the first difference) | Tyre hooks, `Tyre::step`; `addTyreForcesV10` body; `addGroundContact` body | 152,494 bytes of pseudo-C (2,544 lines for `Tyre::step`); 10,040 B; 4,532 B | all |
| CSP's `getPhysicsState` view (low-passed `acc_g` / wheel speed, plane, spline) | `+0x590220` | 3,076 B | all (view only) |
| car collision callback | `+0x587cc0` | 2,946 B | all |
| CSP's extended air in the oracle (track config) | `0x2821430` | unknown (C4a: grows without end) | RSS 3 |

### Part 3

- **CI's Embree.** `web_pack` builds Embree because it depends on `rustyac-web`, which depends on
  `rustyac-game`, which turns `rustyac-ode`'s `embree` on for x64 Windows; `web_pack` itself only packs
  content and never casts a ray. Not cut (moving the feature out of `rustyac-game` would touch every
  desktop build). Instead `build.rs` reads `RUSTYAC_EMBREE_LIB` (the folder to build into and link from),
  and `ci.yml` points it at `$GITHUB_WORKSPACE/target/embree-3.13.4-mt`: the workspace builds it, and
  `csp_oracle` and `web_pack` find it there. A cache miss builds it once instead of three times. The
  release stays one uncached build.
- **CSP mode's shortcuts on the installed content** (no drive; C5i-c refused both: such a session runs
  Vanilla's core with CSP's data and says so, [C5i-c part 1](#part-1-strut-cars-and-loose-track-objects-refused-not-ported)):
  - **Strut suspension asserts** (`PhysicsCore::create_slider_joint` / `create_ball_joint`, "CSP mode has
    no slider joint yet"): `acd_check --strut` lists 55 of the 207 installed cars: abarth500,
    abarth500_s1, alfa_romeo_giulietta_qv(_le), bmw_1m(_s3), bmw_m3_e30 (+ _drift, _dtm, _gra, _s1),
    bmw_m3_e92 (+ _drift, _s1), bmw_m3_gt2, bmw_z4 (+ _drift, _gt3, _s1), ks_abarth500_assetto_corse,
    ks_alfa_mito_qv, ks_alfa_romeo_155_v6 (front and rear), ks_alfa_romeo_4c (rear), ks_audi_a1s1,
    ks_audi_sport_quattro (+ _rally, _s1; front and rear), ks_audi_tt_cup, ks_audi_tt_vln,
    ks_bmw_m235i_racing, ks_bmw_m4 (+ _akrapovic), ks_ford_escort_mk1, ks_ford_mustang_2015,
    ks_porsche_718_boxster_s (+ _pdk) and _cayman_s (front and rear), ks_porsche_911_carrera_rsr,
    _911_gt3_cup_2017, _911_gt3_r_2016, _911_gt3_rs, _911_r, _935_78_moby_dick, _991_carrera_s,
    _991_turbo_s, ks_porsche_cayman_gt4_clubsport / _std (front and rear), ks_ruf_rt12r (+ _awd),
    ks_toyota_ae86 (+ _drift, _tuned), ks_toyota_celica_st185 (front and rear), ks_toyota_gt86,
    ruf_yellowbird.
  - **Loose track objects not built** (`track_survey --csp`, the loose-objects column): 37 of the 66
    installed layouts have some: ks_barcelona (3 layouts, 13), ks_black_cat_county (3, 3), ks_brands_hatch
    (2, 20), ks_laguna_seca (33), ks_monza66 (3, 13), ks_nordschleife (endurance 35, endurance_cup 25,
    nordschleife / touristenfahrten 14), ks_nurburgring (4, 21-23), ks_red_bull_ring (3, 11),
    ks_silverstone (4, 24-26), ks_silverstone1967 (41), ks_vallelunga (14 / 10 / 6), ks_zandvoort (8),
    monza (2, 20), mugello (8), spa (2, 6). Spa's 6 are on the rows' track: a row that hits one would
    differ there.

### Vanilla

Vanilla's code paths are unchanged (the fixes only run with `--csp`). `run_all.sh csp physics`: csp 65 of
68 identical (the 3 others are the new rows), physics 121 of 121. `run_all.sh --quick`: every area
identical but the 3 rows and browser's flags.md check, which failed only because the edit had turned
`docs/flags.md` into CRLF (web_pack's test splits on `
## `); LF again, `usage_and_docs` passes.

**Resume here (C5i-b):** see the table at the top.

## Task C5i-c: the C runtime, CSP mode's refusals, the car collision callback, two cars

### Part 0: one C runtime inside the exe (the release blocker)

Since C5i-a linked Embree, `rustyac.exe` and `csp_oracle.exe` needed the Visual C++ redistributable
(`MSVCP140.dll`, `VCRUNTIME140.dll`, `VCRUNTIME140_1.dll` and seven `api-ms-win-crt-*` sets), which the
farm VM (like many players' PCs) doesn't have. Now every Windows MSVC build links the C runtime
statically: `.cargo/config.toml` gives `x86_64-pc-windows-msvc` `-C target-feature=+crt-static` (so CI
and the release get it too, and every tool under `tools/`), and `crates/rustyac-ode/build.rs` builds
Embree with `/MT` into `<target>/embree-3.13.4-mt` (the release flags on the CMake line:
`CMAKE_MSVC_RUNTIME_LIBRARY` left Embree on `/MD` under CMake 4.4). build.rs stops with a message when the
build has no `+crt-static` (a `RUSTFLAGS` in the environment replaces the config file's), as a mixed
link fails with 8 unresolved `__imp_*` CRT symbols.

| exe | imports before | after |
|---|---|---|
| rustyac.exe | MSVCP140, VCRUNTIME140, VCRUNTIME140_1, api-ms-win-crt-{convert,heap,locale,math,runtime,stdio,string} + system DLLs | system DLLs only (ADVAPI32, KERNEL32, WS2_32, bcrypt, d3d11, d3dcompiler_47, dinput8, dwrite, gdi32, ole32, oleaut32, user32, winmm, xinput1_4, ...) |
| csp_oracle.exe | the same runtime DLLs + ADVAPI32, KERNEL32, ntdll, ole32, bcryptprimitives | ADVAPI32, KERNEL32, ntdll, ole32, bcryptprimitives, synch |

Why static and not shipping the redistributable's DLLs: nothing shares C runtime state with another
module. Rust's allocator is `HeapAlloc` (no `malloc` handed across), the game-side maths that must match
the game go through `msvcr120.dll` loaded at run time (`rustyac-math`), and FMOD, `python33.dll` and CSP's
`dwrite.dll` keep their own runtimes. The maths rustyAC itself takes from the UCRT (`f32::sin`, `powf`, ...
in code that isn't held against the game) now comes from the SDK's static `libucrt` instead of the
machine's `ucrtbase.dll`: same source, and the proofs below (the areas that start `rustyac.exe`) say
whether a bit moved.

On the farm VM (no VC++ redistributable), `run_all.sh --quick --farm dumps launcher fresh sound replays
gui csp` (the areas that failed there with the runtime DLLs missing), 1 h 23 min:

| area | cases | identical | different | errors | before (missing DLLs) |
|---|---|---|---|---|---|
| dumps | 6 | 6 | 0 | 0 | every case ERROR |
| launcher | 1 | 1 | 0 | 0 | every case ERROR |
| fresh | 63 | 63 | 0 | 0 | DIFFERENTs |
| sound | 4 | 4 | 0 | 0 | DIFFERENTs |
| replays | 6 | 6 | 0 | 0 | DIFFERENTs |
| gui | 28 | 28 | 0 | 0 | `hud_cars_simple` DIFFERENT |
| csp | 69 | 61 | 8 | 0 | 47 ERRORs |

- The quick fresh subset's t18 game-replay cases (`t18_magione__ks_ferrari_f2004` lap and kerb strike)
  run and are identical on the VM; whether all 74 t18 cases are back needs a full run (not run here).
  `hud_cars_simple` isn't in the quick gui subset, so whether its frame-0 difference had the same cause
  is open for the next full run.
- csp's 8 differences on the VM: the 4 rows and the strut case named the PC's AC folder (Spa from the
  tape), which the VM doesn't have; csp.sh now passes them through lib.sh's `drive` (a copy naming the
  VM's AC folder), not proven on the VM yet. The 3 `core <car> <n>` cases are a VM difference, not the
  runtime: CSP itself runs to the end there, but its world hash differs from the PC's (`4509fe7bf7076e59`
  against `f363a77722849402`) and the port's loader answered no set-up call ("answers set-up 0"); the
  VM's CPU (Xeon E5620, no AVX) takes other paths in CSP (its Embree switch, ISA dispatch). The csp area
  keeps no results and was only ever proven on the PC; running it on the farm is new.

### Part 1: strut cars and loose track objects: refused, not ported

Sizes: CSP's strut is its own `SuspensionStrut` hooks (constructor, extra links, `setDamage`, step with
the damper marker and hash block, FIX_STRUT_FFB's steer basis: 9,210 bytes of pseudo-C,
[physics_tyres_suspension.md](physics_tyres_suspension.md)) plus a double slider joint the f64 core lacks;
loose objects are the `PhysicsObject` constructor hook (39,244 bytes: collider optimisation, a Barcelona
special case), `setWorldMatrix`, the pose sync `+0x68b330` and `CUSTOM_POBJECTS_MOTION`
([physics_car_core.md](physics_car_core.md)). Neither is small, so CSP mode refuses them:

- `rustyac_physics::csp::choose_core` (called by `GameSim::build` before anything is built, with the
  player's and the opponents' data folders and the track) keeps the session off CSP's core when a car has
  a STRUT suspension (front or rear) or a collider box with a zero side (`CspCore::add_box_collider`
  refuses it), or the track has loose objects (the desktop game builds them, `track_objects`), or the
  track doesn't build in CSP's core. The session then runs as `--csp` did before C5i-a: Vanilla's core
  with CSP's data (`csp::enabled()` still true; `csp::core()` false).
- What the player sees: the game's message banner, title "Custom Shaders Patch", "CSP physics for this
  car isn't ready yet: it runs with Vanilla's physics and CSP's data (<why>)" (`track` for a track), and
  the same line in the log (stderr, `CSP: ...`). The web build has no banner (log only).
- The asserts behind it stay (`create_slider_joint`, `create_ball_joint`, `create_track_object` in CSP
  mode) but no car or track reaches them now.
- The rest of CSP mode's panics, checked: the load-time ones (zero-side box, track core build) are
  `choose_core`'s too. Left: mid-step `Unhandled` collisions in `csp_collision.rs` (a trimesh tree kind,
  a pair other than box/trimesh, a reduction keeping no contact, container/space shapes), "a track geom
  moved" (`csp_core.rs`) and Embree ray errors: none was ever hit by a row, a car or a track can't be
  told from them before the drive, so they are in Not done.
- Note: with `track_objects` on (the desktop game) Spa and 36 other layouts run Vanilla's core in CSP
  mode; the rows and the oracle-made tapes have no loose objects.

Proof: `csp/strut abarth500` (the lap's first 1000 steps, CSP mode, abarth500): the line is printed, the
drive runs to the end (state hash 269aaa221602dd2d), no panic.

### Part 2: CSP's car collision callback `+0x587cc0`, ported

`chassis.rs` `csp_hit` (pure: inputs and the car fields it writes) and `RollingChassis::on_csp_collision`
(what CSP then does with the answer) replace `Car::onCollisionCallBack` in CSP mode. The branch is
general.ini `ALT_COLLISIONS_HANDLING=1` (this install's and CSP's default): CSP's own closing speed from
the contact's point velocities (double, `csp_core` `point_vel`), the other body's mass over the car's
(at most 1), damage zones from the car's box (front, back, +x, -x by the side the point is furthest out
of), suspension damage at most once in 0.1 s, sleeping frames, last collision times, the engine blow-up.
A ground shape (group 1 or 0x10) changes no damage. Shortcuts: only the ALT branch; online CSP's sleeping
car rule (`[ONLINE] CUSTOM_MOTION`) is not there.

Proof, in process: csp_oracle break kind `f` (`CSP_ORACLE_BREAK=587cc0f`) stops at every call of CSP's
callback, reads its inputs and the car's fields, calls the port's `csp_hit` on the same, lets CSP run, and
compares every field CSP wrote, as bits. Every row:

| row | calls | ground | damaging | setDamage | differ |
|---|---|---|---|---|---|
| F2004 lap 4000 | 26 | 26 | 0 | 0 | 0 |
| RSS 3 lap 4000 | 621 | 621 | 0 | 0 | 0 |
| F2004 wall 6667 | 3977 | 3977 | 0 | 0 | 0 |
| two F2004 1600 | 1917 | 443 | 1474 | 590 | 0 |

The rows' first differences didn't move (the callback writes nothing the tyre step's first difference
depends on): the wall row's is still step 154.

### Part 3: does real CSP give Spa a track config? Yes; the port's air is right

This install has `extension/config/tracks/loaded/spa.ini` (visual), and CSP's extended air doesn't hang
on it anyway: CSP's air density hook (`+0x588850`) takes the car's CSP record from the per-car list at
`dwrite+0x28213f0` (or the loading object's +0xe0 while a car loads), and only with a record whose
+0x228 is set uses CSP's law. The list is filled by CSP's car-load finisher (`+0xf35b50`), which runs
in the game for every car and which the oracle never calls (it builds cars through the game's own
constructor, not the loader). So in the game an extended car (RSS 3) at Spa gets CSP's law (1.1128 at
Spa's start), the port's value; the oracle's CSP keeps Vanilla's 1.1856. Pushing the record into the
list by hand crashed the oracle's step (the finisher's other set-up is missing), so the oracle side stays
as it is: the RSS 3 row's first difference is pinned EXPECTED with that reason
(`tools/proofs/expected_skips.txt`), and C5j or a later oracle task that runs the finisher removes it.

### Part 4: two cars

`--physics-states` now writes each car's GUID (the views come in GUID order). A tape can place the oracle
cars: `oracle_gap=<m>` (the opponents at the spawn plus n×gap along the spawn's tail, as csp_oracle's
`CSP_ORACLE_GAP`), and an oracle set-up's opponents are built into the engine (`TrackRun::build_in`;
before, they weren't, and the engine's step asserted). csp_oracle `CSP_ORACLE_PARK=last` parks the last
car on rustyAC's parked driver's controls (brake and clutch 1, the rest 0), so car 0 drives the tape as
rustyAC's player does. Row `csp/row ks_ferrari_f2004 two 1600`: car 0 drives the lap tape, car 1 stands
5.5 m ahead of it (`oracle_gap=-5.5` along the tail) and car 0 drives into it (1474 damaging callback
calls).

| row | steps | verdict | first difference past the view list (pinned) |
|---|---|---|---|
| F2004 lap | 4000 | EXPECTED (644 of 1106 equal) | car 0 step 2 world_matrix[4] 2.0008296e-5 vs 2.005232e-5: CSP's stock tyre step (C5j) |
| RSS 3 lap | 4000 | EXPECTED (610 of 1106) | car 0 step 0 air_density 1.1855999 vs 1.1127756: the oracle's air (part 3) |
| F2004 wall | 6667 | EXPECTED (621 of 1106) | car 0 step 154 world_matrix[3] -0.5623173 vs -0.56231904: CSP's stock tyre step (C5j) |
| two F2004 | 1600 | DIFFERENT (car 0 687, car 1 698 of 1106) | car 1 step 1 world_matrix[3] -0.98705477 vs -0.98706245 (not pinned); car 0's is the lap row's, step 2 world_matrix[4] |
| strut abarth500 | 1000 | IDENTICAL | the message, and the drive runs to its end |

Car 1's first difference is at step 1 (world matrix; body velocity 0.608 vs 0.373 m/s, wheel speeds
different, no tyre load different): the parked car is not the same from its first step. It is not CSP's: the same
row with Vanilla on both sides (csp_oracle `--vanilla`, the tape without `csp=1`) has car 1's first
difference at step 1 too, so it is the second car's set-up (its spawn drop, its controls or its aids
against the oracle's), not found in this task (rule 32). Not pinned: it stays DIFFERENT.

### Pinned first differences (EXPECTED)

`lib.sh` `pinned`: a line `<area> <case> | first <difference> | why` in `expected_skips.txt` makes a row
EXPECTED only while its first difference (past the view list: car, step and value) is that one; an
earlier step or another value is DIFFERENT again. The F2004 lap and wall rows pinned CSP's stock-car tyre
step (since C5j, the later body difference: [C5j](#task-c5j-csps-tyre-step-for-stock-cars)), the RSS 3
row the oracle's air (above).

### Vanilla

Vanilla's code paths are unchanged (CSP mode only: `csp::core()` is false without `--csp`, the callback
runs only on CSP's core's contacts, `oracle_gap` only in an oracle tape). On the PC, `run_all.sh --quick`:
every area identical (physics 9, fresh 63, sound 4, render 18, gui 29, python 1, replays 6, net 16,
browser 18, ai_page 10, shaders 4, launcher 1, dumps 6) but csp (66 identical, 3 EXPECTED rows, the two
row DIFFERENT); `run_all.sh csp physics`: csp the same, physics 121 of 121.

**Resume here (C5i-c):** see the table at the top.

## Task C5j: CSP's tyre step for stock cars

### Part 0: the two-car row's second car

`oracle_gap` put car 1 5.5 m ahead of car 0 at car 0's spawn height, but Spa's road climbs about 19 cm
there: car 1 started sunk (step-0 tyre loads 38370, 24778, 32791, 26678 N against car 0's 767, 0, 0, 0),
and the port and CSP push a sunk car out differently. Not its parked controls. Tape key
`oracle_lift=<m>` (csp_oracle `CSP_ORACLE_LIFT`) lifts each opponent by n×lift; 0.18 m still sinks it at
step 2, 0.2 m lands it at step 22. Row `two` uses 0.2: car 1 now matches to step 171 (859 of 1106
equal; before, step 1, 698). Open question: a sunk lone car differs at steps 1-3 too (the port's and the
game's push out of a deep start), not looked at.

### Part 1: the survey

Every function of CSP's tyre region (`+0x69d320`..`+0x6acff0`) as a plain break (`re/scratch/taskC5j/survey.sh`),
F2004 and RSS 3, steps 0-2: both cars take the same path, every tyre every step. CSP's `Tyre::step`
(`+0x6a0570`, called from the car step `+0x58bccc`) finds no tyre extension for a stock car (`+0xf2f660`
gives null) and calls the original `Tyre::step` (`0x140283800`, slot `0x1827f9780`); in there each hooked
call finds no extension either and calls the original through its slot. The doc's claim (build 4184,
`physics_tyres_suspension.md`) that stock cars get CSP's extended tyre is wrong for 4305 in the oracle.

| hook (call site in acs) | address | size | reached (F2004, RSS 3) | differs from Vanilla's | what it changes | estimate |
|---|---|---|---|---|---|---|
| Tyre::step (`+0x58bccc`) | `+0x6a0570` | 0x56d8 | both, every tyre | no: calls the original; the after-call (`+0xf4cd50`) has no scope object, `+0x6a5808` only copies `tyre+0x2f4` out | nothing | - |
| addGroundContact (`acs+0x283f54`) | `+0x6a70c0` (body `+0x6a7190`) | 0x11f0 | both | no: original | - | 1 session (extended cars) |
| addTyreForcesV10 (`acs+0x283f99`) | `+0x6a8410` (body `+0x6a84a0`) | 0x2738 | both | no: original | - | 2 sessions (extended cars) |
| acos helper (`acs+0x27f07c`) | `+0x6a8380` | 0x8a | both | no: TLS `+0x188` null, the original | - | - |
| getCorrectedD (`acs+0x27f2b8`) | `+0x69ee20` | | both, steps ≥ 1 | no: original | - | - |
| stepDirtyLevel (`acs+0x27f3b2`) | `+0x6a65e0` | | both | no: original, then nothing | - | - |
| updateAngularSpeed (`acs+0x28426a`, non-driven fronts) | `+0x69e640`, tail `+0x69e80d` | 0x28d | both, 2 tyres | **yes**: CSP's own tail | sign test counts 0 as +1, so a wheel starting from 0 doesn't lock | ported |
| updateLockedState (from the tail; driven rears `acs+0x28427d`) | `+0x6a0290` | | both | no: original | - | - |
| stepThermalModel (`acs+0x284331`) | `+0x6a6940` (body `+0x6a69a0`) | 0x63d | both | no: original | - | 1 session (extended cars) |
| blankets (`acs+0x284ef5`) | `+0x6a6fe0` | | both | no: original | - | - |

Breaks on rvas ending in a-f trap: `arm_breaks` reads a last letter that is a kind as the kind, so
`…a`/`…b`/`…d`/`…e`/`…f` are not plain breaks (use the next instruction's address). Every lowercase
letter is now a kind; C5j's are uppercase.

**The difference.** CSP's tail (no extension): `w = torque/inertia·dt + w`; lock when
`(w_new >= 0) != !(0 > w_old)`; `w_old = w_new`; locked → 0; `|w| < 1` (or NaN) → ×0.9. Vanilla's
`sign()` gives 0 for 0, so at step 1 (`w_old` 0, `w_new` 0.0194) Vanilla locks the front-left and CSP
doesn't: the row's step-1 `wheel_angular_speed[1]` (0.0175 vs 0) and step-2 world matrix.

### Part 2: the port

`rustyac_physics::tyre::angular_speed_tail` is the tail after `updateLockedState`, with `csp` the CSP
sign test (`crate::csp::enabled()` in the step). In process: csp_oracle break kinds `U` (`+0x69e818`,
after `updateLockedState`: runs the port on the tyre's inputs) and `V` (`+0x69e8a4`, the end: speed, old
speed and lock as bits against the port's), `CSP_ORACLE_BREAK=69e818U,69e8a4V`: F2004 300 steps, 600 of
600 tails same.

| row | steps | before | now |
|---|---|---|---|
| F2004 lap | 4000 | step 2 world_matrix[4] | step 63 world_matrix[3] -0.98427385 vs -0.98427343 (644 of 1106) |
| F2004 wall | 6667 | step 154 world_matrix[3] | the same |
| two F2004 | 1600 | car 0 step 2 | car 0 step 165 world_matrix[3] (car 1 step 171) |
| RSS 3 lap | 4000 | step 0 air_density (the oracle's air) | the same |

At the lap row's step 63 the body (velocity, angular velocity, suspension and world matrices, cg height)
differs first; tyre loads, slips and forces only from step 64. So the next difference is not in a tyre's
outputs: the car step's other CSP hooks or the core. The three F2004 rows are pinned at these steps.

### Plan (sessions)

1. C5j part 2 continued (1 session): break the car step's other CSP hooks at steps 62-63 (suspension,
   drivetrain, aero, `+0x58b790`'s own components) the same way, port the first that differs.
2. Extended tyres (CSP's `addTyreForcesV10`, `addGroundContact`, thermal bodies): only for cars with a
   tyre extension, none of the proof cars; 3-4 sessions when one is chosen.

### Vanilla

`update_angular_speed` keeps Vanilla's sign test without `--csp`. On the PC: `run_all.sh --full physics`
121 of 121 identical; `--full csp` 66 identical, 4 EXPECTED (the rows above); `--quick` every other area
identical (fresh 63, render 18, gui 29, net 16, dumps 6, shaders 4, replays 6, sound 4, python 1,
launcher 1) but browser 17 of 18 (render scale 100 % to 50 %: an edge along the middle column, 17
against 4); browser again alone, 18 of 18.

### Session 2: the body difference

**The lap rows' set-up.** `oracle/game/track_spa_lap.ryin` has no `oracle_collide` keys, so rustyAC
replayed the F2004 lap row with collisions off (`replay.rs`: `chassis.collisions_enabled = self.collide.on`)
while the game always collides; at world step 62 the game has 3 chassis-vs-track contact joints. `csp.sh`
now adds the wall tape's collide keys (`oracle_collide=1 oracle_collider_mesh=1 oracle_mesh_bounce_vel=0000028b
oracle_no_collision_steps=0 oracle_floor=0`) to the lap and two rows' rustyAC tapes. With them, every core
call (`CSP_CORE_TRACE`) is equal through step 164.

**The sleep limit.** At step 165 the port's chassis velocity was zero: Vanilla's sleeping rule
(`sleepingFrames > framesToSleep`, 50) stopped the car, the game's did not. CSP's `Car::step` wrapper
`+0x58afc0` sets `framesToSleep` (Car+0x3e28) to 250 before the original, every step: other cars directly
(`+0x58b108`), the car with GUID 0 at the start of `+0x58a8d0`. That function then applies
`[PHYSICS_EXPERIMENTS] DISABLE_AUTOBRAKING` (general.ini, byte `0x28031e8`; 0 "Original" by default,
1 realistic, 2 adapted, 3 auto-braking: 500 frames, `sleepingFrames` capped at 498, flag 0x10000) and
a second setting (byte `0x2803218`+6, probably `AWAKE_IN_PITS`, not checked: within 1 m of a stored
position, `sleepingFrames = 0`). In the oracle (mode 0, not
in the pits) only the 250 stays (breaks: `+0x58ae06` and the return hit, `+0x58ac05` / `+0x58ae17` /
`+0x58af5e` not). Ported in `RollingChassis::step_car_begin` under `csp::enabled()`; the other modes are a
`shortcut:` comment. Break-tool note: plain breaks on instructions with a RIP-relative operand
(`+0x58ae0e`, `+0x58ae4a`, `+0x58af42`) never logged a hit here; break the next instruction.

`actions_state` joined the rows' view list (CSP's getPhysicsState fills it; rustyAC's view does not).

| row | steps | session 1 | now |
|---|---|---|---|
| F2004 lap | 4000 | step 63 world_matrix[3] | step 1417 tyre_grain[1] 4.08e-11 vs 0 (1012 of 1106 equal; nothing else differs) |
| F2004 wall | 6667 | step 154 world_matrix[3] | step 1414 tyre_grain[1] 5.46e-11 vs 0 (988; nothing else differs) |
| two F2004 | 1600 | car 0 step 165 | step 620 both cars (car 0 meets the parked car 1; port damage 14.09, game 0) |
| RSS 3 lap | 4000 | step 0 air_density | the same (pinned) |

`tyre_grain`: core temperatures, loads, slips and forces are equal at every step; CSP does not hook
`Tyre::stepGrainBlister` (`0x140284600`), but does hook `Tyre::setCompound` and `Tyre::reset`, so the
grain inputs (`GRAIN_THRESHOLD`, `GRAIN_GAIN`) or the surface are the suspects. The two-car contact: every
collision callback (`+0x587cc0`) equals the port's `csp_hit`, so it is in the contact itself.

**RSS 3 tail (U/V).** `COL=1 BRK=69e818U,69e8a4V rowc.sh rss_formula_rss_3_v6_2026 600`: 1200 tails, all same.

**RSS 3 air: more than small.** The finisher `+0xf35b50` (from `+0xf36950`, the car-extension step) was
run for its physics part (`re/scratch/taskC5j/finisher.patch`: car+0x3db4 bits, car+0x3e9c list index, the
per-car record pushed to `0x28213f0`, `+0x7ef3a0`). With a record, more CSP code wakes up, each needing a
session object the oracle does not have: the air hook reads the avatar's RaceEngineer (`avatar+0xf80`,
stand-in fixed it); `stepDirtyLevel` (`+0x6a65e0`) reads `record+528` and runs CSP's dirt/wet code
(`+0x8cc4c0`, scales the tyre's grip factors `+768/+772/+796/+800`) on the per-car array `0x28177c0`
(sized at session start by `+0x676560`; built by hand, fixed); then a fault in acs `SplineLocator::step`
(`FuelLapEvaluator::step`, a null CSP spline object via `+0x936db0`). Stopped there. So the part-1 survey's
"stepDirtyLevel: original, then nothing" holds only without a record: a car with CSP's record (RSS 3)
gets CSP's dirt code, which the port lacks too.

**Not now (written down).** CSP's own tyre code for cars with a tyre extension (3-4 sessions; neither
proof car has one). The sunk lone car's push-out difference at steps 1-3.

### Plan (sessions, after session 2)

1. `tyre_grain` (1 session): break the grain inputs in process (CSP's `setCompound` result: thresholds,
   gains), port what differs.
2. The two-car contact at step 620 (1 session): `CSP_CORE_TRACE` both cars around it.
3. RSS 3 air and dirt (2-3 sessions): the oracle builds CSP's session objects (spline, dirt arrays,
   RaceEngineer) so the finisher can run; then port CSP's dirt code.

### Session 3: neither difference was in the physics

**`tyre_grain`: the record, not the step.** csp_oracle break kind `G` (`CSP_ORACLE_BREAK=6a0570G`, at
CSP's `Tyre::step` hook: wheel, `GRAIN_THRESHOLD`, gamma, `GRAIN_GAIN`, load, ndSlip, core temperature,
hub velocity, grain, surface gripMod, consumption rate) gave CSP's inputs; the port's
`step_grain_blister` computes the same grain from them (4.083846691933005e-11 at step 1417, wheel 0).
The port's `--physics-states` never wrote it: `CarView::physics_state` left `tyre_grain` and
`tyre_blister` at 0. `RenderState` now carries `grain`/`blister` per wheel (`view.rs`). The lap and wall
rows: nothing past the view list differs in 4000 / 6667 steps; their pins are gone.

**The two-car contact: the oracle's collider.** Without `CSP_ORACLE_LOADER`, csp_oracle gives the game's
car an 8-vertex stand-in box (0.6 × 0.2 × 1 m) as its collider mesh (main.rs `build_car`, C5i-a); the
port has the car's `collider.kn5`. The two row never set it, so from step 620, when car 0 reaches car
1, the two worlds met different shapes (1315 callbacks, 491 `setDamage` with the box; 2043, 0 with the
real mesh). With `CSP_ORACLE_LOADER=use` (every row now), nothing past the view list differs in 1600
steps, both cars; its pin is gone. The callback check could not see it: it compares CSP's answer with
the port's on CSP's own inputs.

**Rows verdict.** A row whose values past the view list are all equal is IDENTICAL (the check had
wanted every one of the 1106 equal, the view list's too, which no row can be).

**Cases follow the installed content.** "refused off/csp" want the listed cars that are installed;
a refused car not in a list passes (named: "new (add to the lists in csp.sh)"), a listed one no longer
refused fails. "layouts" wants the listed `*_fa_2026` layouts that are installed; a layout not in the
list passes when the survey without `--csp` refuses it as "made for Custom Shaders Patch only" and
loads it with `--csp` (named), any other changed row fails. With Super's `pxw_sl_v4` and
`acu_mexico_2021`: all three IDENTICAL, naming them.

**Resume here (C5j):** see the table at the top.
