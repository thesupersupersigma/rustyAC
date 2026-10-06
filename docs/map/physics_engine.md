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
| Library | Open Dynamics Engine (ODE) | String `ODE INTERNAL ERROR %d`, `LCP internal error, s <= 0 (s=%.4e)`, RTTI `dxBody`, `dxWorld`; 609 ODE functions (957 public names) in `acs.pdb` (`dWorldStep` @ `0x1403404c0`, `dBodyCreate` @ `0x14033ef50`, …) |
| Version | **0.13.1** | The PDB's source-file table lists 26 headers under `d:\dev\dev_pc_master_race\ode-0.13.1\include\ode\` (`version.h`, `objects.h`, `threading.h`, …). Consistent with the code: `dJointCreateDBall` and the `dxStepperProcessingCallContext` stepper only exist from 0.13 on, and `dxJoint::getInfo2(float, float, Info2Descr const*)` is the pre-0.16 signature |
| Precision | single (`float`) | Library file is `ode_single.lib`, built from the solution configuration `ReleaseSingleLib`; config string ends in `ODE_single_precision` |
| Build options | `ODE ODE_EXT_no_debug ODE_EXT_trimesh ODE_EXT_opcode ODE_OPC_16bit_indices ODE_OPC_new_collider ODE_EXT_mt_collisions ODE_EXT_threading ODE_single_precision` | The literal `dGetConfiguration()` string in `.rdata`; the game prints it at start-up as `ODE BUILD FLAGS: %s` |
| Meaning of those options | no debug checks; triangle meshes on, through the **OPCODE** collision library, with **16-bit triangle indices** and the newer mesh-mesh collider; thread-local collision caches (OU library); threading interface compiled in but **without** ODE's built-in thread pool (`ODE_THR_builtin_impl` is absent) | ODE's `ode.cpp` builds the string from those compile switches |
| Linking | static | No physics DLL in the import table (25 DLLs: D3D, FMOD, Steam, CRT, Windows). The PDB lists 90 object files from `…\acs\x64\ReleaseSingleLib\ode_single.lib`; only **70** of them have code left in the image (609 functions, 343 k bytes). The other 20 (`quickstep`, `hinge`, `hinge2`, `universal`, `amotor`, `lmotor`, `piston`, `pr`, `pu`, `plane2d`, `transmission`, `dhinge`, `timer`, …) were stripped completely by the linker |
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
| `IJoint` | `BallJointODE`, `DistanceJointODE`, `SliderJointODE`, `FixedJointODE` | one ODE joint (wrapper is 0x10 bytes; `DistanceJointODE` is 0x18 and also keeps the rod length measured at creation). Two methods besides the destructor: `release` (+0x00) and `setERPCFM(erp, cfm)` (+0x08) |
| `ICollisionObject` | `CollisionMeshODE` (static track mesh), `RBCollisionMesh` (mesh attached to a body) | one ODE triangle mesh |
| `IRayCaster` | `RayCaster` (0x18 bytes; `rayCast` at +0x08) | a private ODE ray for repeated ray casts |

`ksPhysicsCoreODEFactory::create` @ `0x1402cb630` is the only place a `PhysicsCore` is built.
Exactly **98 ODE API functions** are called from Kunos code, all of them from those 5 object
files (re-counted from `re/index/calls.tsv`; full list with callers: run
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
| Shared ray | length 100, first-contact = 1, backface cull = 1 (closest-hit is never switched on: `dGeomRaySetClosestHit` is not linked). Its category/collide bits are left at ODE's default (all ones) | `dCreateRay`, `dGeomRaySetFirstContact`, `dGeomRaySetBackfaceCull` |
| QuickStep iteration count | **48**, set but never used (the game never calls QuickStep) | last call of the constructor: `0x1403404b0(this->id, 0x30)`. That address carries two merged names, `dGeomSetCategoryBits` and `dWorldSetQuickStepNumIterations` (both are `mov [rcx+0x78], edx`); the first argument here is the **world**, so in source it is `dWorldSetQuickStepNumIterations(world, 48)`. It is *not* a category mask on the ray |
| Start-up | `dInitODE2(0)`, `dAllocateODEDataForThread(~0)`; the physics thread repeats the second call through `initMultithreading` (vtable +0x08, `0x1402ccc60`) | |

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

- **`dWorldStep`, not `dWorldQuickStep`.** No QuickStep code is linked at all (`quickstep.obj`
  contributes zero functions; `dWorldStep` @ `0x1403404c0` hands `dxStepIsland` to
  `dxProcessIslands`), so there is **no iteration count that matters** and no SOR parameter. The
  only QuickStep trace is the setter `dWorldSetQuickStepNumIterations`, which shares its 4 bytes
  of machine code (and address `0x1403404b0`) with `dGeomSetCategoryBits`. The constructor does
  call it once on the world with 48 (see 3.1); all other calls to that address pass a shape and
  are `dGeomSetCategoryBits`. The value 48 is stored in the world and never read.
- Step size is the constant `0.003` s passed down from `PhysicsDriveThread` (see `docs/map/tyre.md`).
- Collision detection alternates: car-vs-car contacts (everything in `spaceDynamic` against
  itself, which also covers movable track objects) are rebuilt on even frames, car-vs-track
  contacts on odd frames. Each set of contact joints therefore lives for **two** steps. The
  parity comes from `PhysicsCore::currentFrame`, which starts at 0 and is only incremented inside
  `collisionStep`, so steps skipped by `noCollisionCounter` do not advance it.
- `setNoCollisionSteps(n)` (vtable +0x98) skips collision detection for `n` steps. The contact
  groups are **not** emptied while it counts down, so contacts that already exist keep acting
  unless `resetCollisions()` (vtable +0x00, `0x1402cd570`, empties both groups) was called first.
  The only caller found is the "session info received" handler of `PhysicsAvatar`
  (`0x1401213f0`): `PhysicsEngine::setSessionInfo` @ `0x140264560` → `resetCollisions()`, then
  `setNoCollisionSteps(250)` (0.75 s). Nothing in the pseudo-C calls it when a car is teleported.

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
| hub, one per wheel (none for AXLE wheels) | `Suspension::Suspension` @ `0x1402c0720`, `SuspensionStrut::SuspensionStrut` @ `0x1402c38d0` (hub + strut body), `SuspensionML::loadINI` @ `0x1402c9970` | the unsprung mass; the tyre pushes on this |

So a typical car is one ODE island of 6 bodies (body, tank, 4 hubs); strut cars add one strut body per strut corner. A live-axle car has **no rear hub bodies**: its two `SuspensionAxle` objects (Left, Right) share the single `Car::rigidAxle` body (`SuspensionAxle::SuspensionAxle` @ `0x1402c6b90` creates no body; `SuspensionAxle::getMass` @ `0x1402c84c0` reports half the axle mass per side), so it is body, tank, 2 front hubs (plus strut bodies) and 1 axle.

### 3.4 Joints

| ODE joint | Wrapper / factory (IPhysicsCore slot) | Used for |
|---|---|---|
| **DBall** (fixed distance between two points; 1 constraint row) | `createDistanceJoint` +0x28 → `dJointCreateDBall` | every suspension link: `Suspension::attach` @ `0x1402c0fb0` creates **5** (four wishbone arms + steering rod); `SuspensionStrut::attach` @ `0x1402c4100` creates 3; `SuspensionAxle::SuspensionAxle` @ `0x1402c6b90` and `SuspensionML::loadINI` @ `0x1402c9970` create theirs in a loop. `reseatDistanceJointLocal` +0x50 (`0x1402cd480`) moves the anchors (setup changes, steering rod): it takes the two anchors in **body-local** coordinates, converts them with `dBodyGetRelPointPos`, and then puts back the rod length that was measured when the joint was created (`dJointSetDBallDistance(joint, wrapper+0x10)`), so a reseat never changes the length of a rod |
| **Slider** (5 rows; a 6th only with a limit or motor, which the game never sets) | `createSliderJoint` +0x68 (normalises the axis itself), `setSliderAxis` +0x70 | strut body ↔ hub in `SuspensionStrut` |
| **Ball** (3 rows) | `createBallJoint` +0x78 | car body ↔ strut top in `SuspensionStrut` |
| **Fixed** (6 rows) | `createFixedJoint` +0x80 | fuel tank ↔ body |
| **Contact** (always 3 rows here: 1 normal + 2 friction, because `mu` > 0 and `dContactMu2` is off; the only inequality rows) | created inside `PhysicsCore::onCollision` | body ↔ track, car ↔ car |

Row counts were checked in the binary (`getInfo1`): DBall writes m = 1 / nub = 1, Ball 3 / 3,
Fixed 6 / 6, Slider 5 / 5. Anchors given to `createDistanceJoint` and `createBallJoint` are in
**world** coordinates.

No hinge, hinge2, universal, motor or spring joints are linked. **Springs, dampers, bump stops and
anti-roll bars are not ODE objects**; they are forces computed by Kunos code (see
`docs/map/suspension.md`). `createBumpJoint` (+0x30) still exists in the interface and is called
by `Suspension::attach` / `SuspensionStrut::attach`, but its body is the shared stub
`0x14039a0b0` (`xor eax, eax; ret`): it returns null and `bumpStopJoint` stays empty.

Joint softness is changed at run time, but less widely than it looks. Confirmed from
`Car::step` (test at `0x140275e1a`):

- The whole block runs **only for the car whose `physicsGUID` is 0**, i.e. the first car created
  (`physicsGUID` = number of cars already in the engine when the car is built). Every other car
  keeps what its suspension constructor set once at load (`Suspension::Suspension` @ `0x1402c0720` ends with `setERPCFM(0.3, baseCFM)` on its 5 joints and `SuspensionAxle::SuspensionAxle` @ `0x1402c6b90` calls `SuspensionAxle::setERPCFM(0.3, baseCFM)`; the strut and multi-link constructors make no ERP/CFM call, so their joints keep the world values 0.3 / 1e-7. `baseCFM` is always 1e-7, so every type starts with the same numbers and only car 0 ever sees ERP 0.9, below 1 m/s).
- For that car: if the body's squared speed is ≥ 1 (so ≥ 1 m/s) each suspension gets
  `setERPCFM(0.3, baseCFM)` (ISuspension slot +0xb8, `baseCFM` at ISuspension +0x30) and the
  fuel-tank joint `setERPCFM(0.3, −1)`; otherwise `setERPCFM(0.9, 1e-7)` and the fuel-tank joint
  `setERPCFM(0.9, −1)`. The −1 means "leave CFM alone".
- Slot +0xb8 is implemented by `Suspension` (`0x1402c3230`), `SuspensionStrut` (`0x1402c61b0`)
  and `SuspensionAxle` (`0x1402c8680`), which loop over their joints. For **`SuspensionML` it is
  the empty function** `0x140017870`: multi-link joints never change.
- `IJoint::setERPCFM` ignores values ≤ 0 and calls `dJointSet…Param(dParamERP = 0xd /
  dParamCFM = 8)`. For DBall, Ball and Fixed this is one merged function (`0x140340e10`) that
  writes the joint's own `erp` / `cfm`. For the **Slider** it lands in
  `dxJointLimitMotor::set` @ `0x14034ddb0`, which returns at once for any parameter above 10: the
  ERP is silently dropped, and the CFM goes to the limit-motor's `normal_cfm`, which only matters
  for a slider with a limit or motor. So a strut's slider keeps the world ERP 0.3 / CFM 1e-7 for
  its 5 rows whatever `setERPCFM` is given (the last sentence is from the ODE source, not re-read
  in `dxJointSlider::getInfo2`).

### 3.5 Forces in, state out

Everything Kunos computes enters ODE as an external force or torque on a body before
`dWorldStep`: `addForceAtPos` (+0x110), `addForceAtLocalPos` (+0xf8), `addLocalForce` (+0xd8),
`addLocalForceAtPos` (+0xe8), `addLocalForceAtLocalPos` (+0xf0), `addTorque` (+0x118),
`addLocalTorque` (+0xe0) → `dBodyAdd…`. State is read back with `getWorldMatrix` (+0x48),
`getVelocity` (+0x78), `getAngularVelocity` (+0xa8), `getPointVelocity` (+0x108),
`getLocalPointVelocity` (+0x100), `localToWorld` / `worldToLocal` (+0x50 / +0x58) and the
`…Normal` variants. `addLocalForce` is `dBodyAddRelForceAtRelPos(force, 0,0,0)`.
`getWorldMatrix` copies ODE's 3×4 rotation **transposed** into the game's `mat44f`
(`M12 = R[4]`, `M21 = R[1]`, …, position in `M41..M43`); `setRotation` does the reverse.

Mass is set with `setMassBox` (+0x38, `0x1402ce890`: `dMassSetBoxTotal(mass, lx, ly, lz)`, so
`car.ini [BASIC] INERTIA` is a box size, not an inertia) or `setMassExplicitInertia` (+0x00,
`0x1402ce910`). The second one is **not** a clean diagonal inertia: it writes the three values to
`dMass.I[0]`, `I[4]` and `I[8]` (struct offsets 0x14, 0x24, 0x34), which is the diagonal of a
3×3 matrix, but ODE's `dMatrix3` is 3×4 and its diagonal is `I[0]`, `I[5]`, `I[10]` (the offsets
0x14, 0x28, 0x3c that `getLocalInertia` reads back). The result is not positive definite, and
`dBodySetMass` @ `0x14033fb30` then falls back to an **identity** inverse inertia
(`_dInvertPDMatrix` fails → `dRSetIdentity`). It has two callers: `Car::initCarData` @ `0x140272b30` when `car.ini` has an `[EXPLICIT_INERTIA]` section, and `Car::updateBodyMass` @ `0x140276c70` (end of `Car::Car`, then once per 1000 ms of physics time) whenever `bodyInertia` is (0,0,0), which is the state that section leaves behind; the second call passes `Car::mass` as is (hub masses not subtracted, ballast ignored) and prints `explicit` (see `docs/map/body.md` 5.1). None of the 113 cars in `cardata/` has the section, so the path is dead in practice. Copy it as is, do not "fix" it.

A stopped car is frozen by hand, not by ODE's auto-disable: after `framesToSleep` quiet steps
`Car::step` calls `stop()` (+0x70) on `body` and on `fuelTankBody` (not on the hubs), which
zeroes that body's velocities and accumulated forces (`RigidBodyODE::stop` @ `0x1402cead0`; its
float argument is ignored). ODE's own auto-disable is used only for movable track objects:
`PhysicsObject::PhysicsObject` @ `0x1402ac8a0` builds a body with a box mass and one mesh
collider, then calls `setAutoDisable(true)` (+0x18) and `setEnabled(false)` (+0x10).

Wrapper methods with no behaviour: `PhysicsCore::setRigidBodyIterations` and
`reseatDistanceJointLength` (IPhysicsCore +0x58 / +0x60, which is which cannot be told) and
`RigidBodyODE::removeCollisionObjects` (+0x20) and `addSphereCollider` (+0xd0) are all the shared
empty function `0x140017870`.

### 3.6 Collisions, contacts and rays

- **Shapes.** Track: static triangle meshes (`CollisionMeshODE`, `dCreateTriMesh`, 16-bit
  indices, so at most 65 536 vertices per mesh) in `spaceStatic`. Car: boxes (`RigidBodyODE::addBoxCollider`
  @ `0x1402cddb0`) and optional triangle meshes (`addMeshCollider` @ `0x1402ce080`) in
  `spaceDynamic`. The only shapes ever created are ray, box and triangle mesh (`dCreateRay`,
  `dCreateBox`, `dCreateTriMesh` are the only shape constructors linked); cylinder / capsule /
  sphere / convex / heightfield code is linked because ODE's collider table references it.
  Rays are only ever tested against `spaceStatic`, which holds nothing but meshes, so ray-box
  never runs. `createCollisionMesh` (+0x38) accepts a matrix and a body but passes neither on:
  `CollisionMeshODE::CollisionMeshODE` @ `0x1402cebb0` copies the vertices as given (world
  coordinates) and builds the mesh with 12-byte vertices and 6-byte triangles.
- **Filtering.** `nearCallback` tests category/collide bit masks both ways (both
  `cat1 & col2` and `cat2 & col1` must be non-zero); asks for at most 4 contacts when both shapes
  have bodies, else 32. The masks, read at the call sites (details belong to `docs/map/body.md`
  and `docs/map/track_surface.md`):

  | Shape | Category | Collides with | Space | Set in |
  |---|---|---|---|---|
  | car box (`colliders.ini`) | 4 | 1 | per-car sub-space `physicsGUID + 1` | `CarColliderManager::loadINI` @ `0x1402a37a0` |
  | car mesh | 4 | `0x1e`; `0x1a` with pit collisions off; 2 for a car named `spectator`; value 1 (bit 0, the track-surface category) OR-ed in while the body matrix `M22` < 0.25 | per-car sub-space `physicsGUID + 1` | `Car::initColliderMesh` @ `0x140273b20`, then every step `Car::updateColliderStatus` @ `0x140276df0` |
  | movable track object mesh | `0x10` | `0x1f` while its body is enabled, `0x0c` while it is disabled | `spaceDynamic` itself | `PhysicsObject::PhysicsObject`, then every step its `evOnStepCompleted` lambda `0x1402acb00` |
  | remote (network) car mesh | 8, or 0 while ghosted | 4; 0 for a car named `spectator` | see `docs/map/body.md` 5.5 | `NetCarStateProvider::NetCarStateProvider` @ `0x140116c70`, `NetCarStateProvider::step` @ `0x14011bd00` |
  | track mesh | `SurfaceDef::collisionCategory` | `0x14` | static sub-space given by the caller | `Track::addSurface` @ `0x140277e50` |

  With these numbers two car boxes can never pass the filter (4 & 1 = 0), and neither can a box
  against another car's mesh. Unless something changes a box mask later (`setBoxColliderMask`,
  +0xc0, has no caller in the pseudo-C), **box-box is never used**: car-to-car contact is
  mesh-mesh, and a box only ever meets track meshes whose category has bit 0 (value 1) set.
- **Contact material** (`PhysicsCore::onCollision`), default: `mode = 0x7014`
  (`dContactApprox1 | dContactSoftCFM | dContactBounce`), friction `mu = 0.25`,
  `bounce = 0.01`, `soft_cfm = 1e-4`. Box against triangle mesh (ODE class 1 with class 8, in
  either order): `mode = 0x701c` (adds `dContactSoftERP`), `mu = 0.1`, `bounce = 0`,
  `soft_erp ≈ 0.714` (`0x3f36db6e`, 5/7), `soft_cfm ≈ 9.52e-4` (`0x3a79a934`, 1/1050), and the
  contact is **dropped unless** the contact normal, expressed in a body's frame, has `y ≥ 0.9`
  (i.e. the box only rests on things below it). Read in the disassembly: the body is the first
  shape's body, or the second shape's if the first has none; the test is `comiss y, 0.9f` /
  `jae keep`, so a NaN drops the contact; if neither shape has a body the game prints
  `Warning, box collision with no body attached` and keeps the contact. Interpretation: 5/7 and
  1/1050 are exactly what ODE's spring formulas give for a spring of 250 000 N/m with a damper
  of 300 N·s/m at h = 0.003 (`erp = h·k/(h·k+c)`, `cfm = 1/(h·k+c)`).
  The `dSurfaceParameters` offsets used (mode +0x00, mu +0x04, bounce +0x18, soft_erp +0x20,
  soft_cfm +0x24, contact geometry at +0x40) match the 0.13 layout with the three rolling-friction
  fields, and `dxJointContact::getInfo1` tests bit `0x400` (`dContactRolling`), also new in 0.13.
  **`bounce_vel` (+0x1c) is never written** although `dContactBounce` is set: the joint copies
  whatever was on the stack, and `dxJointContact::getInfo2` does read it (joint +0xac) when the
  bounce flag is on. See open question 6.
- **Tyres are not ODE contacts.** A tyre casts one ray (`PhysicsCore::rayCast` @ `0x1402cd1a0`
  sets the length of the shared ray, `RayCaster::rayCast` @ `0x1402cee90` uses its own ray; both
  end in the private overload @ `0x1402cd070` → `dGeomRaySet` → `dSpaceCollide2(ray, spaceStatic)`
  → `rayNearCallback` @ `0x1402cd210`) and turns the hit into forces itself (`docs/map/tyre.md`).
  The callback skips any shape that has a body, asks `dCollide` for one contact per mesh and
  keeps the one with the smallest distance, so the nearest hit wins **between meshes**. Inside
  one mesh the ray runs with first-contact on and closest-hit off, which in ODE's source means
  OPCODE stops at the first triangle it finds, not necessarily the nearest (open question 7).
- After each contact is created the game's own callback (`ICollisionCallback` slot +0x08 =
  `PhysicsEngine::onCollisionCallBack` @ `0x140264020`, registered in the `PhysicsEngine`
  constructor through `setCollisionCallback`, +0x48) is told which bodies/shapes touched, the
  normal, the position and the depth (damage, sounds). Dropped contacts are not reported.

### 3.7 How much ODE code is actually linked

| Part | Functions | Code bytes | Needed for |
|---|---|---|---|
| Dynamics core (`ode`, `objects`, `util`, `step`, `joint`, `mass`, `rotation`, `odemath`, `matrix`, `lcp`, `fastdot/ldlt/lsolve/ltsolve`, plus the small helpers `array`, `obstack`, `error`, `memory`) | 168 | 46.6 k | every step of every car |
| The five joints (`ball`, `dball`, `fixed`, `slider`, `contact`) | 49 | 10.1 k | suspension, fuel tank, contacts |
| Collision kernel, spaces, primitive shapes | 140 | 81.4 k | only the dispatch, the simple space and the box / ray shape objects are really used; box-box and ray-box are linked but, by the bit masks in 3.6, never reached |
| Triangle-mesh colliders (`collision_trimesh_*`) | 42 | 33.1 k | track and car meshes |
| OPCODE / Ice (AABB trees, ray/OBB/tree colliders) | 159 | 166.1 k | every tyre ray, every body-track test |
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
2. **Stage 2 — contacts** (`dxJointContact`, Dantzig LCP with bounds and friction). Box-box is
   not needed (3.6: the bit masks never let two boxes meet). The game's only contact sources are
   box-mesh and mesh-mesh from Stage 3, so until then the contact points for tests have to be fed
   in by hand through the oracle (`dJointCreateContact` with a hand-made `dContact`).
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
| `PhysicsCore::rayCast` (public, shared ray) / `rayCast` (private, any ray) / `rayNearCallback` | `0x1402cd1a0` / `0x1402cd070` / `0x1402cd210` |
| `PhysicsCore::resetCollisions` / `setNoCollisionSteps` / `initMultithreading` / `getDynamicSubSpace` / `getStaticSubSpace` | `0x1402cd570` / `0x1402cd660` / `0x1402ccc60` / `0x1402cc920` / `0x1402ccac0` |
| `PhysicsCore::reseatDistanceJointLocal` / `setSliderAxis` | `0x1402cd480` / `0x1402cd670` |
| `setERPCFM` for DBall, Ball and Fixed (one merged function) / for Slider | `0x1402cd5a0` / `0x1402cd600` |
| `RigidBodyODE::setMassBox` / `setMassExplicitInertia` / `getLocalInertia` / `getWorldMatrix` | `0x1402ce890` / `0x1402ce910` / `0x1402ce480` / `0x1402ce6c0` |
| `CollisionMeshODE::CollisionMeshODE` / `RayCaster::RayCaster` / `RayCaster::rayCast` | `0x1402cebb0` / `0x1402cedc0` / `0x1402cee90` |
| ODE `dBodySetMass` / `dJointSetBallParam` (= DBall, Fixed) / `dJointSetSliderParam` / `dxJointLimitMotor::set` | `0x14033fb30` / `0x140340e10` / `0x140341a00` / `0x14034ddb0` |
| ODE `dGeomSetCategoryBits` = `dWorldSetQuickStepNumIterations` (merged) | `0x1403404b0` |
| ODE `getInfo1` of DBall / Ball / Slider / Fixed / Contact | `0x140342070` / `0x140340cf0` / `0x140341110` / `0x140341b90` / `0x14034e810` |
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

1. **Float mode of the ODE build.** Checked on one function only: `_dDot` @ `0x140390530` is
   plain scalar SSE (`mulss`/`addss`), not vectorised, and adds the products in exactly the order
   of ODE's `fastdot.c` (the compiler only unrolled the tail loop by four, keeping the order).
   That is what strict floating point (`/fp:precise`, the compiler default) produces, so the
   0.13.1 source order can most likely be followed, with the disassembly as the referee. Still to
   check the same way when porting starts: `_dFactorLDLT`, `_dSolveL1`, `_dSolveL1T` and
   `dxStepBody`.
2. **`dWorldStep` internals were described from the 0.13.1 source**, not re-read from the binary.
   The island ordering (which decides the row order of the matrix, and therefore rounding) needs
   an instruction-level pass when Stage 1 starts.
3. **Threaded islands.** The threading interface is compiled in but the game never installs a
   thread pool (`dWorldSetStepThreadingImplementation` is not linked), so ODE should run its
   default single-threaded path. `PhysicsEngine` has its own `ThreadPool` for `Car::step` only.
   Worth confirming that island processing order is fixed.
4. **Contact groups look swapped** (`contactGroup` is refreshed on car-vs-car frames,
   `contactGroupDynamic` on car-vs-track frames). It is harmless but should be copied as is.
5. **The `y ≥ 0.9` rule for box-vs-mesh contacts** is now confirmed in the disassembly (3.6):
   first shape's body, else the second's; kept when `y ≥ 0.9f`. Still open: ODE's normal points
   into the *first* shape, so the rule only makes sense if the car's box is always the first
   shape handed to `nearCallback`. The order in which `dSpaceCollide2(spaceDynamic, spaceStatic)`
   and its sub-space recursion deliver the pair was not traced.
6. **Uninitialised `bounce_vel`.** `PhysicsCore::onCollision` builds its `dContact` on the stack
   and never writes `surface.bounce_vel` (+0x1c), but sets `dContactBounce` in both modes. In ODE's
   source bounce is applied only when `bounce_vel >= 0` and the closing speed exceeds it, so with
   `bounce = 0.01` (default material: mesh-mesh, i.e. car against car, wall or object) the result
   depends on stale stack bytes left by `dCollide`. For box-vs-mesh `bounce` is 0, so the value
   cannot matter there. A port needs either the real stack value (measure it with the oracle) or
   a decision to accept a tiny difference in car-to-car impacts.
7. **Tyre ray inside one mesh.** First-contact is on and closest-hit is off (3.1). By ODE's
   source the OPCODE ray collider then stops at the first triangle it reaches in tree order.
   Not verified in the binary (`dCollideRTL` @ `0x14038aa90` was not read). If true, a tyre ray
   that crosses two layers of the same mesh needs the exact OPCODE tree order to match the game.
8. **Per-step ERP/CFM only for car 0.** Read as fact from the code (3.4), but the reason is
   unknown and it means the first car created (presumably the player's, not checked) and all
   other cars use different joint softness below 1 m/s.
9. **`[EXPLICIT_INERTIA]`** gives a broken inertia (3.5). No shipped car uses it, so it was not
   measured in a running game; mods that use it would behave as described only if ODE's fallback
   is really reached, which is read from `dBodySetMass` but not run.
10. **Not verifiable from the local index:** the solution configuration name and the 90-object
    count were re-read straight from `acs.pdb` (read-only); the import-table count (25 DLLs) was
    re-read from `acs.exe` (read-only). All three hold.
