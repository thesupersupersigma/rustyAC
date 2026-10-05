# What moves the car body: the rigid-body engine inside acs.exe (read-only)

Source: `acs.exe` (PE timestamp 2018-01-10, game version 1.16.4) + `acs.pdb`, read with the local
tools (`tools/pdb_index.py`, `tools/xref_index.py`, `tools/disasm.py`, `tools/bulk_decomp.py`).
Raw pseudo-C of every function named here: `re/decomp/PhysicsCore/`, `re/decomp/RigidBodyODE/`,
`re/decomp/CollisionMeshODE/`, `re/decomp/RayCaster/` (git-ignored). Nothing in the game folder, the
Ghidra project or a running game was changed. No Rust was written.

---

## 1. Plain-English summary

Assetto Corsa does not have its own code for "a heavy object flying through space". It borrows a
free, open-source physics library called **ODE** (Open Dynamics Engine), **version 0.13.1**, and
the library is baked directly into `acs.exe` (there is no separate DLL).

The library does three jobs for the game. First, it keeps track of every solid object: where it
is, which way it is turned, how fast it moves and spins, and how heavy it is. A car is not one
object but several: the main body, a separate fuel tank bolted to it, and one small "hub" object
for each wheel. Second, it holds those objects together with rigid rods and hinges. A double
wishbone suspension, for example, is literally five rods of fixed length between the body and the
hub, exactly like the real thing; the library works out, every step, the forces in those rods
that keep the wheel where the rods allow it to be. Third, it finds out when solid things touch
(car body against the ground, a wall or another car) and pushes them apart, and it answers the
tyres' question "what is directly below me?" by shooting a straight line at the track surface.

Everything that makes a car feel like a car is **not** in the library. Tyre grip, springs,
dampers, anti-roll bars, aerodynamics, engine, gearbox and brakes are all Kunos' own code. Each of
them works out a push or a twist and hands it to the library ("push the hub up with this many
newtons at this point"). Once per step, after every car has handed in all its pushes, the game
asks the library to move the whole world forward by 3 milliseconds.

The game uses the library's slow-but-exact solving method, not its fast approximate one, and it
runs it with ordinary single-precision numbers. That matters for us: the method has no random or
iteration-count-dependent part, so a careful Rust copy can give the same numbers as the game.

---

## 2. Which library, which version, how it is linked

| Question | Answer | Evidence |
|---|---|---|
| Library | Open Dynamics Engine (ODE) | String `ODE INTERNAL ERROR %d`, `LCP internal error, s <= 0 (s=%.4e)`, RTTI `dxBody`, `dxWorld`; 609 ODE public symbols in `acs.pdb` (`dWorldStep` @ `0x1403404c0`, `dBodyCreate` @ `0x14033ef50`, …) |
| Version | **0.13.1** | The PDB's source-file table lists 26 headers under `d:\dev\dev_pc_master_race\ode-0.13.1\include\ode\` (`version.h`, `objects.h`, `threading.h`, …). Consistent with the code: `dJointCreateDBall` and the `dxStepperProcessingCallContext` stepper only exist from 0.13 on, and `dxJoint::getInfo2(float, float, Info2Descr const*)` is the pre-0.16 signature |
| Precision | single (`float`) | Library file is `ode_single.lib`, built from the solution configuration `ReleaseSingleLib`; config string ends in `ODE_single_precision` |
| Build options | `ODE ODE_EXT_no_debug ODE_EXT_trimesh ODE_EXT_opcode ODE_OPC_16bit_indices ODE_OPC_new_collider ODE_EXT_mt_collisions ODE_EXT_threading ODE_single_precision` | The literal `dGetConfiguration()` string in `.rdata`; the game prints it at start-up as `ODE BUILD FLAGS: %s` |
| Meaning of those options | no debug checks; triangle meshes on, through the **OPCODE** collision library, with **16-bit triangle indices** and the newer mesh-mesh collider; thread-local collision caches (OU library); threading interface compiled in but **without** ODE's built-in thread pool (`ODE_THR_builtin_impl` is absent) | ODE's `ode.cpp` builds the string from those compile switches |
| Linking | static | No physics DLL in the import table (25 DLLs: D3D, FMOD, Steam, CRT, Windows). `ode_single.lib` contributes 90 object files to the PDB |
| Compiler | Visual C++ 2013 x64 (same as the rest of the game; maths from `MSVCR120.dll`) | Library paths `Microsoft Visual Studio 12.0`; ODE code imports `MSVCR120!sinf`, `cosf`, `atan2f` |
| Other engines | none | No PhysX / Bullet / Havok / Newton strings, symbols or imports |

### Kunos' wrapper around it

All ODE calls in the whole executable come from one small library, **`ksPhysicsCoreODE.lib`**
(5 object files, 93 functions, source folder `d:\dev\dev_pc_master_race\ksphysicscoreode\`).
The rest of the game only sees abstract interfaces (declared in `ksphysicsac\iphysicscore.h`):

| Interface | Implementation | What it wraps |
|---|---|---|
| `IPhysicsCore` | `PhysicsCore` (vtable `0x140500600`) | one ODE world, two collision spaces, contact handling, joint and body factories, ray casts |
| `IRigidBody` | `RigidBodyODE` (vtable `0x1405009c0`, 0x48 bytes) | one `dBody` plus its collision shapes |
| `IJoint` | `BallJointODE`, `DistanceJointODE`, `SliderJointODE`, `FixedJointODE` | one ODE joint; only method besides the destructor is `setERPCFM(erp, cfm)` |
| `ICollisionObject` | `CollisionMeshODE` (static track mesh), `RBCollisionMesh` (mesh attached to a body) | one ODE triangle mesh |
| `IRayCaster` | `RayCaster` | a private ODE ray for repeated ray casts |

`ksPhysicsCoreODEFactory::create` @ `0x1402cb630` is the only place a `PhysicsCore` is built.
Exactly **98 ODE API functions** are called from Kunos code (full list with callers: run
`tools/xref_index.py`, then look for `ode_single.lib` callees in `re/index/calls.tsv`). Because
the linker keeps only what is referenced, the ODE code present in `acs.exe` *is* the feature list.

---

## 3. What the car uses from ODE

### 3.1 World settings — `PhysicsCore::PhysicsCore` @ `0x1402cba80`

| Setting | Value | Call |
|---|---|---|
| Gravity | (0, **−9.806**, 0) m/s² (float `0xc11ce560`) | `dWorldSetGravity` |
| Global ERP (how much joint error is corrected per step) | **0.3** | `dWorldSetERP` |
| Global CFM (constraint softness) | **1e-7** | `dWorldSetCFM` |
| Max speed at which contacts may push objects apart | 3.0 m/s | `dWorldSetContactMaxCorrectingVel` |
| Contact surface layer | 0 | `dWorldSetContactSurfaceLayer` |
| World damping (linear, angular) | 0, 0 | `dWorldSetDamping` |
| Collision spaces | two `dSimpleSpace`s: `spaceStatic` (track) and `spaceDynamic` (cars), each with optional sub-spaces keyed by an integer | `dSimpleSpaceCreate` |
| Shared ray | length 100, first-contact = 1, backface cull = 1, category bits `0x30` | `dCreateRay`, `dGeomRaySet…` |
| Start-up | `dInitODE2(0)`, `dAllocateODEDataForThread(~0)` | |

Per body — `RigidBodyODE::RigidBodyODE` @ `0x1402cd800`: `dBodySetFiniteRotationMode(body, 1)`
with finite-rotation axis (0,0,0) (the more accurate rotation update, which is where ODE calls
`sinf`/`cosf`), linear and angular damping 0, user data = the wrapper object.

### 3.2 The step

```
PhysicsEngine::step(dt = 0.003)              0x140264760    (333.33 Hz)
├─ evOnPreStep handlers
├─ Track::step, PhysicsEngine::stepWind
├─ Car::stepPreCacheValues  (each car)
├─ Car::step                (each car; all forces/torques are pushed into the bodies here)
├─ core->step(dt)           PhysicsCore::step @ 0x1402cd690   (IPhysicsCore vtable +0x10)
│   ├─ if noCollisionCounter == 0: PhysicsCore::collisionStep @ 0x1402cbf90
│   │     even frame: empty contactGroup,        dSpaceCollide (spaceDynamic)               car ↔ car
│   │     odd  frame: empty contactGroupDynamic, dSpaceCollide2(spaceDynamic, spaceStatic)  car ↔ track
│   │     └─ nearCallback @ 0x1402ccc70 → dCollide → PhysicsCore::onCollision @ 0x1402ccda0
│   │           → dJointCreateContact + dJointAttach, then the game's collision callback
│   └─ dWorldStep(world, dt)   @ 0x1403404c0      ← the exact ("big matrix") stepper
└─ evOnStepCompleted handlers
```

- **`dWorldStep`, not `dWorldQuickStep`.** No QuickStep code is linked at all, so there is **no
  iteration count** and no SOR parameter. `dWorldSetQuickStepNumIterations` appears in the symbol
  table only because its 4 bytes of machine code are identical to `dGeomSetCategoryBits` and the
  linker merged them; every "call" to it is really `dGeomSetCategoryBits`.
- Step size is the constant `0.003` s passed down from `PhysicsDriveThread` (see `docs/map/tyre.md`).
- Collision detection alternates: car-vs-car contacts are rebuilt on even steps, car-vs-track
  contacts on odd steps. Each set of contact joints therefore lives for **two** steps.
- `setNoCollisionSteps(n)` (vtable +0x98) skips collision detection for `n` steps (used after
  teleporting a car).

What `dWorldStep` does in 0.13.1 (from the ODE source; the code in `acs.exe` was not re-derived
instruction by instruction): group bodies into islands connected by joints; for each island add
gravity and the gyroscopic torque, ask every joint for its constraint rows (`getInfo1`/`getInfo2`),
build the matrix `A = J·M⁻¹·Jᵀ` (+CFM on the diagonal) and the right-hand side, solve for the
constraint forces with the Dantzig LCP solver (`dSolveLCP` @ `0x140392260`), update velocities,
then move each body (`dxStepBody` @ `0x140353860`) and clear its force accumulators.
A car that touches nothing has **only equality rows** (rods and bolts): in that case the solver
is a plain LDLᵀ factorisation and back-substitution (`_dFactorLDLT` @ `0x1403988d0`,
`_dSolveL1` @ `0x140390610`, `_dSolveL1T` @ `0x140390fd0`) with no pivoting and no friction logic.
The pivoting part of the LCP solver only runs when contact joints exist.

### 3.3 Bodies in one car

| Body | Created in | Notes |
|---|---|---|
| `Car::body` | `Car::Car` @ `0x14026bf00` | the sprung mass; all colliders hang on it |
| `Car::fuelTankBody` | `Car::Car` | bolted to `body` with a **fixed joint** (`Car::initCarData` @ `0x140272b30` → `createFixedJoint`); `Car::fuelTankJoint` |
| `Car::rigidAxle` | `Car::Car` (only for live-axle cars; prints `Creating rigid axle body`) | the axle that `SuspensionAxle` links to the body (details in `docs/map/suspension.md`) |
| hub, one per wheel | `Suspension::Suspension` @ `0x1402c0720`, `SuspensionStrut::SuspensionStrut` @ `0x1402c38d0` (hub + strut body), `SuspensionML::loadINI` @ `0x1402c9970` | the unsprung mass; the tyre pushes on this |

So a typical car is one ODE island of 6 bodies (body, tank, 4 hubs); strut cars add one strut
body per strut, live-axle cars add the axle.

### 3.4 Joints

| ODE joint | Wrapper / factory (IPhysicsCore slot) | Used for |
|---|---|---|
| **DBall** (fixed distance between two points; 1 constraint row) | `createDistanceJoint` +0x28 → `dJointCreateDBall` | every suspension link: `Suspension::attach` @ `0x1402c0fb0` creates **5** (four wishbone arms + steering rod); `SuspensionStrut::attach` @ `0x1402c4100` creates 3; `SuspensionAxle::SuspensionAxle` @ `0x1402c6b90` and `SuspensionML::loadINI` @ `0x1402c9970` create theirs in a loop. `reseatDistanceJointLocal` +0x50 moves the anchors (setup changes, steering rod) |
| **Slider** (5 rows) | `createSliderJoint` +0x68, `setSliderAxis` +0x70 | strut body ↔ hub in `SuspensionStrut` |
| **Ball** (3 rows) | `createBallJoint` +0x78 | car body ↔ strut top in `SuspensionStrut` |
| **Fixed** (6 rows) | `createFixedJoint` +0x80 | fuel tank ↔ body |
| **Contact** (1–3 rows, the only inequality rows) | created inside `PhysicsCore::onCollision` | body ↔ track, car ↔ car |

No hinge, hinge2, universal, motor or spring joints are linked. **Springs, dampers, bump stops and
anti-roll bars are not ODE objects**; they are forces computed by Kunos code (see
`docs/map/suspension.md`).

Joint softness is changed at run time: `Car::step` calls `suspension->setERPCFM` (ISuspension
slot +0xb8) every step, with ERP 0.3 and a per-suspension CFM while the car moves faster than
1 m/s, and ERP 0.9 / CFM 1e-7 when it is nearly stopped; the fuel-tank joint gets the same ERP.
`setERPCFM` maps to `dJointSet…Param(dParamERP = 0xd / dParamCFM = 8)` and ignores values ≤ 0.

### 3.5 Forces in, state out

Everything Kunos computes enters ODE as an external force or torque on a body before
`dWorldStep`: `addForceAtPos` (+0x110), `addForceAtLocalPos` (+0xf8), `addLocalForce` (+0xd8),
`addLocalForceAtPos` (+0xe8), `addLocalForceAtLocalPos` (+0xf0), `addTorque` (+0x118),
`addLocalTorque` (+0xe0) → `dBodyAdd…`. State is read back with `getWorldMatrix` (+0x48),
`getVelocity` (+0x78), `getAngularVelocity` (+0xa8), `getPointVelocity` (+0x108),
`getLocalPointVelocity` (+0x100), `localToWorld` / `worldToLocal` (+0x50 / +0x58) and the
`…Normal` variants. Mass is set with `setMassBox` (+0x38, `dMassSetBoxTotal`) or
`setMassExplicitInertia` (+0x00, diagonal inertia).

A stopped car is frozen by hand, not by ODE's auto-disable: after `framesToSleep` quiet steps
`Car::step` calls `body->stop()` (+0x70), which zeroes the body's velocities and accumulated
forces (`RigidBodyODE::stop` @ `0x1402cead0`).

### 3.6 Collisions, contacts and rays

- **Shapes.** Track: static triangle meshes (`CollisionMeshODE`, `dCreateTriMesh`, 16-bit
  indices, so at most 65 536 vertices per mesh) in `spaceStatic`. Car: boxes (`RigidBodyODE::addBoxCollider`
  @ `0x1402cddb0`) and optional triangle meshes (`addMeshCollider` @ `0x1402ce080`) in
  `spaceDynamic`. Linked colliders: ray-trimesh, box-trimesh, trimesh-trimesh, box-box, ray-box
  (cylinder/capsule/sphere/convex/heightfield code is linked because ODE's collider table
  references it, but nothing in the game creates those shapes).
- **Filtering.** `nearCallback` tests category/collide bit masks both ways; asks for at most 4
  contacts when both shapes have bodies, else 32.
- **Contact material** (`PhysicsCore::onCollision`), default: `mode = 0x7014`
  (`dContactApprox1 | dContactSoftCFM | dContactBounce`), friction `mu = 0.25`,
  `bounce = 0.01`, `soft_cfm = 1e-4`. Box against triangle mesh: `mode = 0x701c` (adds
  `dContactSoftERP`), `mu = 0.1`, `bounce = 0`, `soft_erp ≈ 0.714`, `soft_cfm ≈ 9.52e-4`, and the
  contact is **dropped unless** the contact normal, expressed in the body's frame, has `y ≥ 0.9`
  (i.e. the box only rests on things below it).
- **Tyres are not ODE contacts.** A tyre casts one ray (`PhysicsCore::rayCast` @ `0x1402cd1a0` /
  `RayCaster::rayCast` @ `0x1402cee90` → `dSpaceCollide2(ray, spaceStatic)` →
  `rayNearCallback` @ `0x1402cd210`, nearest hit wins, bodies are ignored) and turns the hit into
  forces itself (`docs/map/tyre.md`).
- After each contact is created the game's own callback (`PhysicsEngine::onCollisionCallBack`
  @ `0x140264020`) is told which bodies/shapes touched, where, and how deep (damage, sounds).

### 3.7 How much ODE code is actually linked

| Part | Functions | Code bytes | Needed for |
|---|---|---|---|
| Dynamics core (`ode`, `objects`, `util`, `step`, `joint`, `mass`, `rotation`, `odemath`, `matrix`, `lcp`, `fastdot/ldlt/lsolve/ltsolve`) | 168 | 46.6 k | every step of every car |
| The five joints (`ball`, `dball`, `fixed`, `slider`, `contact`) | 49 | 10.1 k | suspension, fuel tank, contacts |
| Collision kernel, spaces, primitive shapes | 140 | 81.4 k | only box-box, ray-box and the dispatch are really used |
| Triangle-mesh colliders (`collision_trimesh_*`) | 42 | 33.1 k | track and car meshes |
| OPCODE / Ice (AABB trees, ray/OBB/tree colliders) | 158 | 166.1 k | every tyre ray, every body-track test |
| Threading, TLS, init | 51 | 5.8 k | start-up only |

---

## 4. Port options and recommendation

| Option | Bit-exact with AC? | Effort | Pure Rust? |
|---|---|---|---|
| **(a) Port the needed part of ODE 0.13.1 1:1 into Rust** | **Yes, achievable.** Single-precision SSE2 scalar code from one known compiler, no iteration counts, no randomness in `dWorldStep`; `sinf`/`cosf`/`atan2f` come from `MSVCR120.dll`, which the tyre port already borrows. The operation order must be taken from the disassembly (as for the tyre), with the 0.13.1 source as the guide | **L** overall, but it splits cleanly: dynamics core + 4 joints ≈ 57 k bytes of machine code (**M–L**); contacts + Dantzig pivoting (**M**); OPCODE mesh collision (**L–XL**, 166 k bytes, tree build order matters) | Yes |
| (b) Build ODE 0.13.1 from source and call it over FFI | **Not guaranteed.** Only identical if built with the same compiler generation and float settings and linked against the same maths functions; any modern compiler may vectorise or contract differently. It could be made to match, but proving it needs the same oracle work as (a) | **S** to get running, **M** to chase bit differences | No (C++ dependency, Windows CRT) |
| (c) Run AC's own ODE code in-process (the oracle trick: map `acs.exe`, call `dWorldCreate`, `dBodyCreate`, `dWorldStep` … by address) | Yes, by definition | **S–M** (the public symbols give every entry point; needs `dInitODE2` + TLS) | No — and it cannot ship. **Test tool only** |
| (d) Replace ODE with another engine or a hand-written car-only solver | No. Different solver → different rod forces → the car drifts away from the recording within a few steps | M | Yes |

**Recommendation: (a), in stages, with (c) as the measuring stick.**

1. **Stage 1 — equality-only dynamics** (bodies, mass, finite-rotation integrator, DBall / Ball /
   Slider / Fixed joints, island build, `A = J·M⁻¹·Jᵀ`, LDLᵀ solve). This is everything a car on
   four tyres needs as long as its body does not scrape anything, and it avoids the hardest parts
   of ODE (pivoting, friction, meshes). The tyre rays can keep using an analytic flat plane as in
   the tyre oracle.
2. **Stage 2 — contacts** (`dxJointContact`, Dantzig LCP with bounds and friction, box-box).
3. **Stage 3 — meshes** (OPCODE trees, ray-mesh, box-mesh, mesh-mesh) so real tracks work. Tyre
   rays on a real track need only the ray-mesh part; its result is a hit point and normal, so it
   can be checked on its own.

Option (b) is worth keeping in mind only as temporary scaffolding (to drive a whole car before
Stage 1 is finished); it should not be the end state because it breaks "pure Rust" and does not
by itself give bit-exactness. Option (c) should be built first: an `ode_oracle` that drives AC's
own `dWorldStep` on a small scene (two bodies and a DBall, then a full car island) gives the
golden numbers for every stage.

---

## 5. Key addresses

| Function | Address |
|---|---|
| `ksPhysicsCoreODEFactory::create` | `0x1402cb630` |
| `PhysicsCore::PhysicsCore` / `step` / `collisionStep` | `0x1402cba80` / `0x1402cd690` / `0x1402cbf90` |
| `nearCallback` / `PhysicsCore::onCollision` | `0x1402ccc70` / `0x1402ccda0` |
| `PhysicsCore::rayCast` (public) / `rayNearCallback` | `0x1402cd1a0` / `0x1402cd210` |
| `PhysicsCore::createRigidBody` / `createDistanceJoint` / `createSliderJoint` / `createBallJoint` / `createFixedJoint` | `0x1402cc3b0` / `0x1402cc190` / `0x1402cc3f0` / `0x1402cc010` / `0x1402cc2a0` |
| `PhysicsCore::createCollisionMesh` / `createRayCaster` | `0x1402cc0f0` / `0x1402cc360` |
| `RigidBodyODE::RigidBodyODE` / `addBoxCollider` / `addMeshCollider` / `stop` | `0x1402cd800` / `0x1402cddb0` / `0x1402ce080` / `0x1402cead0` |
| ODE `dInitODE2` / `dWorldCreate` / `dBodyCreate` / `dWorldStep` | `0x140340bc0` / `0x1403401f0` / `0x14033ef50` / `0x1403404c0` |
| ODE `dxProcessIslands` / `dxStepIsland` / `dxStepBody` | `0x1403534f0` / `0x1403501b0` / `0x140353860` |
| ODE `dSolveLCP` / `dLCP::solve1` / `_dFactorLDLT` / `_dSolveL1` / `_dSolveL1T` / `_dDot` | `0x140392260` / `0x1403936f0` / `0x1403988d0` / `0x140390610` / `0x140390fd0` / `0x140390530` |
| ODE `dxJointDBall::getInfo2` / `dxJointBall::getInfo2` / `dxJointSlider::getInfo2` / `dxJointFixed::getInfo2` / `dxJointContact::getInfo2` | `0x140342080` / `0x140340d00` / `0x1403411c0` / `0x140341ba0` / `0x14034e970` |
| ODE `dCollide` / `dSpaceCollide2` / `dCollideRTL` (ray-mesh) / `dCollideBTL` (box-mesh) / `dCollideTTL` (mesh-mesh) / `dCollideBoxBox` | `0x140344120` / `0x1403430a0` / `0x14038aa90` / `0x14038a3d0` / `0x14038c0d0` / `0x1403474b0` |

---

## 6. Open questions

1. **Float mode of the ODE build.** Whether `ode_single.lib` was compiled with strict or fast
   floating point is not visible from symbols. It decides whether the Rust port can follow the
   0.13.1 source order or must follow the disassembly everywhere. To check first:
   `_dDot`, `_dFactorLDLT` and `dxStepBody` against the source.
2. **`dWorldStep` internals were described from the 0.13.1 source**, not re-read from the binary.
   The island ordering (which decides the row order of the matrix, and therefore rounding) needs
   an instruction-level pass when Stage 1 starts.
3. **Threaded islands.** The threading interface is compiled in but the game never installs a
   thread pool (`dWorldSetStepThreadingImplementation` is not linked), so ODE should run its
   default single-threaded path. `PhysicsEngine` has its own `ThreadPool` for `Car::step` only.
   Worth confirming that island processing order is fixed.
4. **Contact groups look swapped** (`contactGroup` is refreshed on car-vs-car frames,
   `contactGroupDynamic` on car-vs-track frames). It is harmless but should be copied as is.
5. **The `y ≥ 0.9` rule for box-vs-mesh contacts** was read from pseudo-C; the exact comparison
   and which shape's body is used should be confirmed in the disassembly before porting contacts.
