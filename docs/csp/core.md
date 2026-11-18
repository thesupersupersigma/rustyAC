# Task C5a: CSP's physics core, surveyed, and the plan to port it

Plan row **CSP-3** follow-up ([plan.md](plan.md), [port_c4.md](port_c4.md)). CSP 0.3.2-preview2, build
4305 ([version.md](version.md)). Every `dwrite+0x…` offset is build 4305's, relative to the DLL's base.
Nothing is ported in C5a: this page is the map and the plan.

## Resume here

| Step | State |
|---|---|
| 0. release v1.1.1 | done (tag `v1.1.1`) |
| 1. IDA survey + breakpoint log in `csp_oracle` | done (this page) |
| 2. plan | done (last section) |

## The core in one paragraph

CSP replaces acs's ODE for every car through the `ksPhysicsCoreODEFactory::create` detour
(`dwrite+0x238c80` → creator `+0x6149c0`, a 13,016-byte `ode_impl_core`). Under it is a **full
ODE in double precision, newer than 0.13.1**, built from CSP's own tree (`C:\Development\ode-fresh`)
with trimesh (OPCODE, 16-bit indices, new collider), libccd (internal, cylinder / capsule / convex
pairs), multithreaded collisions and ODE's threading interface but **no** built-in thread pool: CSP
runs islands on its own job pool. The car-side objects are `cphys::` wrappers around ODE bodies,
geoms and joints. Tyre rays go through CSP's raycaster, which uses Intel Embree 3.13.4 only when the
batch-raycast switch is on; otherwise it is ODE's ray-vs-trimesh.

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
| `ODE_BATCH_RAYCAST` | 0 (forced 1 by a flag at `[+0x2821078]+0x16`) | `BYTE3(+0x28031fc)` |
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
| Used | only by the raycaster when `ODE_BATCH_RAYCAST` is 1 (`rtcIntersect1` in `+0x932900`); otherwise ODE's ray |

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
the default path does not use Embree. Do it only if part 1 shows Embree is on for the cars we prove.

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
  default. Not proven: part 2 finds each contact geom's owner and what `+0x587cc0` changes. Until
  then multi-car CSP runs are not a reference.

## Plan

Each part ends bit-exact against `csp_oracle` running CSP's own code, the way C4a proved the air
density: same inputs through CSP's function and the port, every output compared as bits.

| # | Part | Sessions | Proof |
|---|---|---|---|
| 1 | `csp_oracle` on a real track (Spa, CSP track config stand-in) with breakpoints on the raycaster: which ray path the cars take, whether `ODE_BATCH_RAYCAST` is forced | 1-2 | the run repeats bit-exact; ray path logged |
| 2 | Q3: owner of the origin contacts, fix the stand-in or document it | 0.5-1 | car 0 of three = one car while apart |
| 3 | `rustyac-ode` in double next to float (one generic number type or a second module; the float path's results must not move) | 3-4 | existing ODE proofs unchanged; each ODE function vs CSP's copy in-process, bits |
| 4 | newer ODE pieces CSP uses: threaded quickstep with reordering and its seed, libccd pairs; find the ODE release | 2-3 | per-stage in-process compare |
| 5 | CSP's world step: island builder, island jobs, worker count rule, post-step flags | 1-2 | `steps.txt` identical at workers 0 and 7 |
| 6 | collision: near callback, contact reduction, contact creation, the switches above, car collision callback | 2-3 | contact lists identical per step |
| 7 | `cphys::` wrappers stock cars use: core, rigid bodies, dynamic collider, joints of the suspensions, raycaster, dummy suspension | 2-3 | body states identical after set-up and per step |
| 8 | tyre rays: ODE ray path (1); Embree linked only if part 1 needs it (2-3) | 1-3 | hit points identical |
| 9 | whole-car rows in proof area `csp`, `rustyac --csp` steps CSP's core | 1-2 | rows identical on this PC |
| | **Total** | **14-23** (14-20 without Embree) | |

Later, not counted: COSMIC, car-script limp, ragdoll, repairing / pits joints, virtual surfaces,
hash spaces (all off or unused for stock cars by default).

### Decisions

- Port ODE, link Embree (only if needed). ODE runs in the browser; Embree cannot.
- Prove each part in-process against CSP's own function before the whole car: a whole-car diff
  without them only says "different".
- Proofs on this PC only until a part shows no CPU dependence (Embree, FMA).

### Risks

- The ODE release behind `ode-fresh` may be a CSP-modified tree; parts 3-4 then compare against
  CSP's machine code instead of a source tarball (slower).
- Island jobs on several threads: islands are independent, but CSP's measured-time worker rule
  makes the thread count vary run to run; part 5 must show the result does not depend on it.
- The float-to-double split in `rustyac-ode` could move the float path's last bits; part 3 guards
  with the existing proofs.
- Multi-car CSP references wait for part 2.
