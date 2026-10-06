# Rigid-body core, stage 1: the game's ODE (bodies and joints) in Rust (Task 07)

Code: `crates/rustyac-ode` (new), `crates/rustyac-math` (the C-runtime maths module, moved out of
`rustyac-physics` so that both crates share it), `tools/ode_oracle` (new), `tools/disasm.py`
(new `range` command). Sources: the disassembly of `acs.exe` 1.16.4 (build timestamp
`0x5a55e7a8`) with `acs.pdb`, and the official ODE 0.13.1 source, downloaded for reading into
the git-ignored `re/ode-0.13.1/`. Five read-only research briefs that compare the machine code
with the source function by function are in the git-ignored `re/scratch/ode/`
(`a1_bodies_stage3.md`, `a2_joint_stages.md`, `b_solver.md`, `c_joints.md`, `d_api.md`).
Nothing in the game folder or the Ghidra project was changed, no game process was touched, and
no contact, suspension or other car code was ported.

---

## 1. Plain-English summary

The part of Assetto Corsa that moves solid objects now exists in Rust. It is a borrowed physics
library that keeps track of the car body, the fuel tank and the wheel carriers and holds them
together with the suspension rods, and the Rust copy covers everything a car needs as long as
nothing but its tyres touches the ground. It was translated function by function from the
library's published source, but wherever the game's own machine code does a sum in a different
order than the source text says, the machine code was followed, because the goal is not "about
the same" but the same numbers in every digit. To check it, a helper program runs the game's own
copy of the library and the Rust copy side by side on the same made-up scenes (single objects,
each kind of connection on its own, an F2004-shaped car, a car with strut suspension, sleeping
objects, and a thousand random tangles of objects and connections) and compares every number
after every step: all <!-- MICRO_STEPS:BEGIN -->1,996,043<!-- MICRO_STEPS:END --> steps came out identical. Then the twelve recorded
drives of the game's real F2004 from the previous task were replayed. Starting from the first
recorded moment and given only the pushes the car's parts produced, the Rust copy reproduced
each whole drive, up to a full minute of driving, without a single digit of difference, in all
eleven drives in which the car's floor never touches the road. The twelfth drive is the one
made on purpose with a solid floor under the car: its twelve steps with floor contact need the
part that handles touching objects, which is the next stage, and every other step of it is
identical too. On the way three places turned up where the game's build of the library does not
calculate what its source text says (one of them changes the car's numbers on every single
step), plus one surprise in how the solver is called; all are written down below. A small part
of these checks, with the game's answers stored in files, now runs with the project's ordinary
tests and needs no game. The Rust copy is a little slower than the original (<!-- BENCH_SHORT:BEGIN -->9.29 against 8.07 millionths of a second per step of a whole car<!-- BENCH_SHORT:END -->).

---

## 2. What was ported, what was left, and where the game's build differs from the source

### 2.1 The crate

`crates/rustyac-ode`: Rust, single precision, no Cargo dependency outside the repository. Its
`sinf` and `cosf` (called in the rotation update of every body in every step) come from
`rustyac-math`, the module the tyre port uses: on Windows it calls the game's own C runtime
(`MSVCR120.dll`, loaded at run time) and falls back to Rust's own functions when that DLL is
not installed; `sqrtf` is Rust's. So "the same bits as the game" holds by construction where
the DLL is present, and otherwise as far as the two runtimes agree (no difference was seen in
the 20 million inputs per function tested when that module was written, but that is an
observation).
ODE's pointers became handles (`BodyId`, `JointId`); ODE's intrusive lists are kept exactly,
because their order decides the order of the rows in the constraint matrix and therefore the
rounding: the world's body and joint lists (newest first) and each body's list of joint ends
(newest attached first). The function names follow ODE (`dBodyCreate` is `World::body_create`,
`dJointAttach` is `World::joint_attach`, `dWorldStep` is `World::step` …). A function that is a
function of its own in `acs.exe` names its original and its address in its doc comment; helpers
that the compiler or the source inlines carry the ODE name only, and for a few file-local ones
(the four `Multiply…` helpers of `step.cpp`, `getAnchor` / `getAnchor2`,
`removeJointReferencesFromAttachedBodies`) the address is only in the table below. ODE is used under its BSD-style
licence: `crates/rustyac-ode/LICENSE-ODE`, mentioned in the crate's README.

| File | Content |
|---|---|
| `world.rs` | `World`, `Body`, the body API, the seven add-force functions, the frame-conversion getters |
| `joint.rs` | `Joint`, the four joint types, `dJointAttach`, anchors and parameters, `getInfo1` / `getInfo2` |
| `step.rs` | auto-disable, the island builder, the island stepper (all stages), the gyroscopic torque, `dxStepBody` |
| `matrix.rs` | `L*D*L^T` factor and solve, Cholesky, positive-definite inverse, dot product |
| `mass.rs`, `rotation.rs`, `odemath.rs`, `common.rs` | mass helpers, quaternion and rotation helpers, small vector maths, types |

### 2.2 Ported functions

Addresses are Ghidra addresses in `acs.exe`. Functions without a name in the PDB (file-local
functions of `step.obj`, `util.obj`, `fastldlt.obj` and `ode.obj`) are named as in the ODE
source.

| Area | ODE function (address) |
|---|---|
| World | `dWorldCreate` 0x1403401f0 with `dxWorld::dxWorld` 0x14034cfb0 (defaults), `dWorldSetGravity` 0x1403404a0, `dWorldSetERP` 0x140340490, `dWorldSetCFM` 0x140340420, `dWorldSetDamping` 0x140340450, `dWorldStep` 0x1403404c0 |
| Bodies | `dBodyCreate` 0x14033ef50, `dBodyDestroy` 0x14033f340, `dBodySetPosition` 0x14033fbb0, `dBodySetRotation` 0x14033fc00, `dBodySetLinearVel` 0x14033fb10, `dBodySetAngularVel` 0x14033f8f0, `dBodySetForce` 0x14033fad0, `dBodySetTorque` 0x14033fc90, `dBodySetMass` 0x14033fb30, `dBodySetFiniteRotationMode` 0x14033fa90, `dBodySetFiniteRotationAxis` 0x14033fa40, `dBodySetLinearDamping` 0x14033faf0, `dBodySetAngularDamping` 0x14033f8d0, `dBodyEnable` 0x14033f4b0, `dBodyDisable` 0x14033f4a0, `dBodyIsEnabled` 0x14033f8c0, `dBodySetAutoDisableFlag` 0x14033fa00, `dBodySetAutoDisableAverageSamplesCount` 0x14033f910 |
| Forces | `dBodyAddForce` 0x14033e860, `dBodyAddTorque` 0x14033ef10, `dBodyAddRelTorque` 0x14033ee50, `dBodyAddForceAtPos` 0x14033e8a0, `dBodyAddForceAtRelPos` 0x14033e990, `dBodyAddRelForceAtPos` 0x14033eb00, `dBodyAddRelForceAtRelPos` 0x14033ec70 |
| Getters | `dBodyGetRelPointPos` 0x14033f6c0, `dBodyGetPosRelPoint` 0x14033f5e0, `dBodyVectorToWorld` 0x14033fd60, `dBodyVectorFromWorld` 0x14033fcb0, `dBodyGetPointVel` 0x14033f520, `dBodyGetRelPointVel` 0x14033f780 (position, rotation, velocities and mass are public fields of `Body`) |
| Mass | `dMassSetZero` 0x140346d30, `dMassSetParameters` 0x140346c60, `dMassSetBoxTotal` 0x140346bc0 |
| Joints, common | `dxJoint::dxJoint` 0x14034d3f0, `createJoint<T>` 0x14033e3f0, the start values of the four constructors (`dxJointBall` 0x140340c30, `dxJointSlider` 0x140340e30, `dxJointFixed` 0x140341a10, `dxJointDBall` 0x140341f60), `dJointCreateBall` 0x14033fed0, `dJointCreateDBall` 0x14033ff60, `dJointCreateFixed` 0x14033ff70, `dJointCreateSlider` 0x14033ff80, `dJointAttach` 0x14033fe20 (with the local `removeJointReferencesFromAttachedBodies` 0x14033e7e0), `dJointGetBody` 0x14033ff90, `dxJoint::isEnabled` 0x14034dd80, `dJointSetBallParam` = DBall = Fixed 0x140340e10, joint feedback (the stepper's side of `dJointSetFeedback`) |
| DBall (rod) | `dxJointDBall::getInfo1` 0x140342070, `getInfo2` 0x140342080, `updateTargetDistance` 0x140342590, `dJointSetDBallAnchor1` 0x1403426a0, `dJointSetDBallAnchor2` 0x140342750, `dJointSetDBallDistance` 0x140342800, `dJointGetDBallDistance` 0x140342690, `setRelativeValues` 0x140342560 |
| Ball | `dxJointBall::getInfo1` 0x140340cf0, `getInfo2` 0x140340d00, `setBall` 0x14034e230, `dJointSetBallAnchor` 0x140340de0, `setAnchors` 0x14034de50, `setRelativeValues` 0x140340d60, `getAnchor` 0x14034dc60, `getAnchor2` 0x14034db60 |
| Fixed | `dxJointFixed::getInfo1` 0x140341b90, `getInfo2` 0x140341ba0, `setFixedOrientation` 0x14034e5a0, `dJointSetFixed` 0x140341e30, `computeInitialRelativeRotation` 0x140341b10 |
| Slider | `dxJointSlider::getInfo1` 0x140341110, `getInfo2` 0x1403411c0, `computeOffset` 0x140341000, `computeInitialRelativeRotation` 0x140340f80, `dJointSetSliderAxis` 0x1403419c0, `setAxes` 0x14034e030, `dJointSetSliderParam` 0x140341a00, `dJointGetSliderPosition` 0x1403417c0, `setRelativeValues` 0x140341780, `dxJointLimitMotor::init` 0x14034dd40, `dxJointLimitMotor::set` 0x14034ddb0 |
| Islands | `dInternalHandleAutoDisabling` 0x140353230, the island builder 0x1403527a0 (`BuildIslandsAndEstimateStepperMemoryRequirements`), `dxProcessIslands` 0x1403534f0 (as a plain loop) |
| Stepper | `dxStepIsland` 0x1403501b0 and its stages: Stage0_Bodies 0x140350360 (gravity, world inverse inertia, gyroscopic torque), Stage0_Joints 0x140350b80, Stage1 0x140350eb0, Stage2a 0x140351250, Stage2b 0x1403516a0, Stage2c 0x140351d20, Stage3 0x140352100; the local helpers `MultiplyAdd2_p8r` 0x14034f990, `MultiplySub0_p81` 0x14034fa50, `Multiply1_8q1` 0x14034f4d0 and `MultiplyAdd1_8q1` 0x14034f720 (one loop in the port), `dMultiply0_333` 0x14034fcd0, `dMultiply2_333` 0x14034fe60, `dInvertMatrix3` 0x14034fb10; `dxStepBody` 0x140353860 |
| Solver | the part of `dSolveLCP` 0x140392260 and `dLCP::dLCP` 0x140391cf0 that runs when every row is an equality; `_dFactorLDLT` 0x1403988d0 (with the local `dSolveL1_1` 0x140398fc0 and `dSolveL1_2` 0x140399190), `_dSolveLDLT` 0x14034cdf0, `_dSolveL1` 0x140390610, `_dSolveL1T` 0x140390fd0, `_dVectorScale` 0x14034ce60, `_dDot` 0x140390530 |
| Mass inverse | `_dFactorCholesky` 0x14034b7f0, `_dSolveCholesky` 0x14034ca70, `_dInvertPDMatrix` 0x14034bac0, `_dIsPositiveDefinite` 0x14034bce0 |
| Small maths | `_dSafeNormalize3` 0x14034b410, `_dSafeNormalize4` 0x14034b500 (`dNormalize4` 0x14034b570), `dOrthogonalizeR` 0x14034b580, `dPlaneSpace` 0x14034b6e0, `dRSetIdentity` 0x1403465f0, `dQMultiply0` 0x140346040, `dQMultiply1` 0x140346130, `dQMultiply2` 0x140346220, `dQMultiply3` 0x140346310, `dRfromQ` 0x140346620, `dQfromR` 0x140346400, `dDQfromW` 0x140345f70 |

The mass helpers the task mentions (`dMassTranslate`, `dMassRotate`, `dMassAdjust`) are not
linked into `acs.exe`. The game makes box masses (`dMassSetBoxTotal`) or, for a car with
`[EXPLICIT_INERTIA]` in its files (none of the extracted cars in `cardata` has it), writes the fields of
a `dMass` itself (`RigidBodyODE::setMassExplicitInertia` 0x1402ce910); a caller of the crate
does the same with the public fields of `Mass`.

### 2.3 Left out

| What | Why |
|---|---|
| Contact joints (`dxJointContact`, `dJointCreateContact`, joint groups) and the pivoting part of the LCP solver (`dLCP::solve1`, `transfer_i_…`, `unpermute`, `_dLDLTAddTL`, `_dLDLTRemove`, `_dRemoveRowCol`) | stage 2. An island with a bounded row makes the Rust stepper stop with a message |
| A slider that is at a stop or has a motor force (`dxJointLimitMotor::addLimot` 0x14034d500 beyond its immediate return) | a bounded sixth row (stage 2). The test whether a stop is reached is ported and checked with stops that are set but far away; when one is reached, or the motor has a force, the stepper stops with a message. The game never sets stops or a motor on its sliders |
| Collision: spaces, shapes, triangle meshes, rays (OPCODE) | stage 3. Where ODE tells a body's shapes that the body moved (`dxStepBody`, `dBodySetPosition`, `dBodySetRotation`) or detaches them (`dBodyDestroy`) the port does nothing yet |
| `dWorldDestroy`, `dInitODE2`, `dCloseODE`, `dAllocateODEDataForThread`, the threading and memory-arena code, `dBodySetData` / `dBodyGetData` | not needed in Rust: a `World` is an ordinary value, the stages run one after the other as they do in the game (its ODE runs single-threaded), scratch arrays are kept in the `World` |
| `dMassCheck` 0x140346a20 | only validates, and nobody reads its answer. In `acs.exe` it does run (`dMassSetParameters` ends in a jump to it, and `dBodyCreate` calls that); the port leaves the call out |
| `dWorldSetContactMaxCorrectingVel` 0x140340430, `dWorldSetContactSurfaceLayer` 0x140340440 (the game sets 3.0 and 0), `dWorldSetQuickStepNumIterations` 0x1403404b0 | contact settings: stage 2; the quick stepper is never called |

### 2.4 Where the code in `acs.exe` is not the 0.13.1 source

Found by reading the machine code and confirmed by the oracle: before each of the first four
was applied the corresponding worlds did not match, afterwards they did. Rows 4 and 9 are not
differences from the source (the source does the same); they are in the table because a port
written from a quick reading of the source gets other bits there.

| # | Where | Source says | The game's machine code does | Effect |
|---|---|---|---|---|
| 1 | `_dSolveL1`, the last `n % 4` rows, when they have 12 or more columns | add the products one after the other | per block of twelve: products 0–3 into four lanes of one packed sum, 4–7 into four lanes of a second one, 8–11 one after the other; after the last block `sum + ((w2 + w0) + (w3 + w1))` with `w` = lane-wise sum of the two | **every step of the F2004**: its island has 26 rows, rows 24 and 25 go this way (as for any island with more than 12 rows whose row count is not a multiple of four; by the joint lists in `physics_engine.md` a double-wishbone or strut car without contacts has 26, 38 or 50 rows). A literal port of the source matched only 54 % of the steps of the car layout |
| 2 | `_dSafeNormalize3` | divide the three components by the largest | one reciprocal, three multiplications | the direction of **every suspension rod row** (DBall) and the slider axis. A literal port matched 57 % of the single-rod steps |
| 3 | `dMassSetBoxTotal` | `mass / 12 * (…)` | `mass * 0x3daaaaab` (the f32 nearest to 1/12) | every body's inertia |
| 4 | `dBodySetRotation` | (the source does this too, easy to misread) | the quaternion is taken from the matrix as given, not from the orthogonalised copy that becomes `R` | `R` and `q` disagree in the last bits until the first step |
| 5 | `dQMultiply3` | `-b0*c1 - b1*c0 + b2*c3 - b3*c2` | `(b2*c3 - (b1*c0 + b0*c1)) - b3*c2` (same for the other two components) | sign of an exact zero only; used by fixed and slider joints attached to the world |
| 6 | `_dFactorLDLT` | `sum = 0; sum += q1*q2` | `q1*q2` | sign of an exact zero only (singular matrix) |
| 7 | `_dFactorCholesky`, `_dSolveCholesky` | add the products one after the other | packed sums for 8 or more columns: per block of eight, products 0–3 and 4–7 go into two four-lane sums that are folded as in 1, the remaining products are added one after the other; in `_dSolveCholesky` only in the forward substitution | not reached by the game (both callers, `dBodySetMass` through `_dInvertPDMatrix` and `dMassCheck` through `_dIsPositiveDefinite`, pass 3x3 matrices); ported anyway and checked up to 64 rows |
| 8 | Comparisons | C semantics | NaN is not treated as in C in many tests: `_dSafeNormalize3` (a NaN first component gives (1,0,0)), `dOrthogonalizeR`, `dInvertMatrix3` (a NaN determinant counts as singular, so the gyroscopic torque is dropped), the gravity test (a NaN component is skipped), the DBall length tests, `setFixedOrientation`, `_dFactorCholesky`, the damping setters, `dBodySetFiniteRotationAxis` and `dBodySetFiniteRotationMode`, the "does this body move" tests of the gyroscopic term and of `dxJoint::isEnabled` (a NaN inverse mass counts as a moving body), the slider's stop and motor tests | only with NaN input |
| 9 | `sinc` in `dxStepBody`, `dPlaneSpace` | | the threshold tests are made in double precision (`fabsf(x) < 1.0e-4`, `fabsf(n2) > 0.7071…`); for `sinc` this decides one f32 value differently from an f32 test | finite rotation of every body |
| 10 | `dJointSetDBallDistance` 0x140342800 | does not exist in 0.13.1 | sets the rod length | the game's steering-rod reseat uses it: the linked library is not stock 0.13.1 |

Items 1, 2, 3, 5, 6, 7 and 8 are what Visual C++ does under its fast floating-point model
(inferred from the code; no build file is known). The consequence for later stages: any loop of
the form `sum += a[k] * b[k]` over plain arrays may be a packed sum in this binary, so the
contact and collision code needs the same instruction-level reading.

Things that are in the source but decide bits and are easy to get wrong:

- **`nub` is a count of joints.** The stepper hands `dSolveLCP` the number of fully unbounded
  *joints*, not rows. For any island with a multi-row joint (the fuel tank's fixed joint has six)
  that is less than the row count, so the solver's "everything unbounded" shortcut is not taken
  and the general constructor `dLCP::dLCP` runs. With only equality rows it then finds every
  remaining row unbounded, swaps nothing (each swap is of a row with itself), copies the lower
  triangle, and calls the same factor and solve, so the result is the same; the port does the
  factor and solve directly. Stage 2 has to copy the real thing.
- **Rows are in reverse joint order.** The joints of an island are collected in the order the
  depth-first search finds them (per body: newest attached joint first) and then sorted so that
  unbounded joints come first, which reverses them. For the F2004: fuel-tank joint, then the
  rods of LF, RF, LR, RR in creation order.
- The gyroscopic torque of 0.13.1 is the **implicit** one (a 3x3 system per body per step), not
  `-w x (I w)`.
- A body's inverse inertia comes from a Cholesky factorisation: for a box inertia it is
  `(1 / s) / s` with `s = sqrt(I)`, which is not `1 / I` in the last bit.
- Everything is multiplied by `1 / h`, never divided by `h`.
- The DBall's angular Jacobian is a product with a zeroed matrix: the terms `0 * q` are kept
  and decide the sign of zeros.
- The fixed joint's three orientation rows use the **world's** ERP and CFM, only its three
  position rows use the joint's own. A slider ignores an ERP given to it altogether.
- An inertia that is not positive definite (what `[EXPLICIT_INERTIA]` would produce through the
  game's wrapper) gives an identity inverse inertia while the bad matrix stays in use for the
  gyroscopic term. Ported as is and covered by the random worlds.

### 2.5 How the game sets things up (what a caller of the crate has to copy)

World: gravity (0, −9.806, 0), ERP 0.3, CFM 1e-7, damping 0 (`World::assetto_corsa()`; the two
contact settings the game also makes belong to stage 2). Every
body: `dBodySetFiniteRotationMode(1)`, axis (0,0,0), damping 0, so flags 0x101 (finite rotation,
gyroscopic). Masses are boxes for every shipped car. Joints are created, attached, then given their anchors in world
coordinates. Creation order of the F2004 (it decides the island order): car body, fuel tank,
fixed joint (tank, body), then per wheel LF, RF, LR, RR the hub and its five rods (body, hub).
The replay creates the six bodies in that order and then the 21 joints in that order, and gets
the game's island order and row order (both are compared in every step: hub RR, car body,
fuel tank, hub LF, hub RF, hub LR; fuel-tank joint, then the rods of LF, RF, LR, RR).

---

## 3. Results

### 3.1 Micro-oracle: the game's own ODE against the Rust port on synthetic worlds

`tools/ode_oracle micro` maps `acs.exe` into its own process (the game never starts), calls
`dInitODE2`, and then builds every world twice from one list of operations: in the game's ODE
by calling its functions at their addresses (`dWorldCreate`, `dBodyCreate`, `dBodySetMass`,
`dJointCreateDBall`, `dJointAttach`, `dWorldStep` …), and in `rustyac-ode`. World settings are
the game's (gravity −9.806, ERP 0.3, CFM 1e-7, step 0.003 s) except in a part of the random
worlds (below). Before every step both get the same forces through all seven add-force
functions and, now and then, what the game does from outside: a change of joint softness, a
reseat of a rod (the game's `reseatDistanceJointLocal`), a "stop". About half of the worlds with
joints (none of the `auto_disable` worlds) run the way the game does, with no feedback buffer on
any joint (the notes under the table give the counts); in the others every joint has one, so that the constraint forces can be
compared, and before every step it is filled with a value neither engine computes (a force that
a step does not write must be left alone by both). Compared bit for bit:

- after set-up: mass, inertia, inverse inertia and inverse mass of every body, its pose, and
  every joint's parameters (anchors, relative rotation, offset, rod length, ERP, CFM);
- after every step: every body's position, quaternion, rotation matrix, linear and angular
  velocity, accumulators, its place in its island (ODE's `tag`) and flags; the six
  frame-conversion getters for a point that moves from step to step; every joint's parameters
  (for a slider also the nine limit-motor parameters, `dJointGetSliderPosition` and whether it
  is at a stop), flags, its place in the island's joint order (`tag`, which fixes the row
  order), the two bodies `dJointGetBody` returns, and the constraint force (on both bodies).

A step that differs is counted and the Rust bodies' poses and velocities and the joints' own
parameters are put back on the game's (flags and auto-disable counters are not), so that a
difference in the numbers does not spoil the later steps; "worlds bit-exact to the end" counts
the worlds that never needed that, up to their last step or, for the few worlds that are ended
early (below), up to that point.
Every value the game's ODE produced is also folded into a hash per world type (the recorded
state; `--record <dir>` writes the raw values to files).

<!-- MICRO:BEGIN -->
| World type | What it covers | Worlds | Steps | Steps bit-exact | Worlds bit-exact to the end | Set-up bit-exact | Largest matrix, rows | Hash of the game's states | First difference |
|---|---|---|---|---|---|---|---|---|---|
| `free_body` | one free body: gravity, forces, torques, gyroscopic term, rotation update | 200 | 200000 | 100 % | 100 % | 100 % | 0 | `0600b8190c9b1325` | none |
| `ball` | one ball joint (two bodies, or one body and the world) | 100 | 100000 | 100 % | 100 % | 100 % | 3 | `c04b22ef8ab1d69c` | none |
| `dball` | one fixed-length rod (DBall) | 100 | 100000 | 100 % | 100 % | 100 % | 1 | `88295f3f315d462f` | none |
| `dball_zero` | a rod of length zero: the fallback directions of the DBall row | 100 | 100000 | 100 % | 100 % | 100 % | 1 | `ef9677e56446beae` | none |
| `fixed` | one fixed joint | 100 | 100000 | 100 % | 100 % | 100 % | 6 | `855711cb3dab6440` | none |
| `slider` | one slider joint; some with every motor parameter set and with stops far away | 100 | 100000 | 100 % | 100 % | 100 % | 5 | `49ae7b91b9ff1886` | none |
| `auto_disable` | damped chains with ODE's auto-disable on; bodies fall asleep, are woken and disabled | 100 | 100000 | 100 % | 100 % | 100 % | 22 | `01eb945ed4817095` | none |
| `car` | F2004 layout: body, fuel tank, 4 hubs, 20 rods, 1 fixed joint; springs, tyres, steering | 20 | 100000 | 100 % | 100 % | 100 % | 26 | `2ea5894b71fb719b` | none |
| `strut_car` | strut car: body, tank, 4 hubs, 4 strut bodies; per corner a slider, a ball joint and 3 rods | 20 | 100000 | 100 % | 100 % | 100 % | 50 | `c2df1a8b5aef6606` | none |
| `random` | 1 to 8 bodies, random chains of all joint types, loops, several islands; half of them also use what the game never does (other world settings, kinematic bodies, disabled joints, re-attached joints) | 1000 | 996043 | 100 % | 100 % | 100 % | 34 | `a33649b8581bcd5c` | none |
| **all** | | **1840** | **1996043** | **100 %** | | | | | |

Notes from the run:

- `ball`: 50 of the 100 worlds ran as the game does, without joint feedback buffers
- `dball`: 50 of the 100 worlds ran as the game does, without joint feedback buffers
- `dball_zero`: 50 of the 100 worlds ran as the game does, without joint feedback buffers
- `dball_zero`: the rod's anchors were less than 1e-7 m apart in 56423 joint-steps (the row takes the direction of the anchors' relative velocity); in 25921 of them that velocity was below 1e-7 too (last resort: the direction (1, 0, 0))
- `fixed`: 50 of the 100 worlds ran as the game does, without joint feedback buffers
- `slider`: 50 of the 100 worlds ran as the game does, without joint feedback buffers
- `auto_disable`: bodies were disabled (asleep) in 9082 body-steps
- `car`: 10 of the 20 worlds ran as the game does, without joint feedback buffers
- `strut_car`: 10 of the 20 worlds ran as the game does, without joint feedback buffers
- `random`: 500 of the 1000 worlds ran as the game does, without joint feedback buffers
- `random`: 26 of the 1000 worlds reached a value that is not finite; 11020 steps had a NaN in the reference state (NaN against NaN counts as equal)
- `random`: 11 of the 1000 worlds were ended early, in the step in which a slider reached one of its far stops, because ODE then adds a bounded row (stage 2); 3957 steps were not run for that reason
- `random`: bodies were disabled (asleep) in 29728 body-steps
- `random`: joints were in no island (disabled, detached by a destroyed body, between sleeping bodies or with no moving body) in 479812 joint-steps

Calls made on both engines, per world type (how often each kind of call was made):

- `free_body`: before the first step: body created 200, mass set 200, position set 200, rotation set 200, linear velocity set 200, angular velocity set 200, finite-rotation mode/axis set 185, damping set 200. Between steps: force/torque added 119803, body stopped 432.
- `ball`: before the first step: body created 167, mass set 167, position set 167, rotation set 167, linear velocity set 167, angular velocity set 167, finite-rotation mode/axis set 157, damping set 167, joint created 100, joint attached 100, ball anchor set 100, joint parameter set 42. Between steps: force/torque added 100109, body stopped 331, joint parameter set 1594.
- `dball`: before the first step: body created 167, mass set 167, position set 167, rotation set 167, linear velocity set 167, angular velocity set 167, finite-rotation mode/axis set 156, damping set 167, joint created 100, joint attached 100, rod anchor set 200, rod length set 9, joint parameter set 47. Between steps: force/torque added 100045, body stopped 347, joint parameter set 1483, rod reseated 1331.
- `dball_zero`: before the first step: body created 175, mass set 275, position set 175, rotation set 175, linear velocity set 275, angular velocity set 275, finite-rotation mode/axis set 175, damping set 175, joint created 100, joint attached 100, rod anchor set 200. Between steps: force/torque added 22505, rod anchor set 200000, joint parameter set 1489.
- `fixed`: before the first step: body created 167, mass set 167, position set 167, rotation set 167, linear velocity set 167, angular velocity set 167, finite-rotation mode/axis set 155, damping set 167, joint created 100, joint attached 100, fixed joint set 100, joint parameter set 38. Between steps: force/torque added 100430, body stopped 304, joint parameter set 1484.
- `slider`: before the first step: body created 167, mass set 167, position set 167, rotation set 167, linear velocity set 167, angular velocity set 167, finite-rotation mode/axis set 156, damping set 167, joint created 100, joint attached 100, slider axis set 100, joint parameter set 341. Between steps: force/torque added 100198, body stopped 316, joint parameter set 1411.
- `auto_disable`: before the first step: body created 322, mass set 322, position set 322, rotation set 322, linear velocity set 644, angular velocity set 644, finite-rotation mode/axis set 322, damping set 644, auto-disable switched 322, body enabled/disabled 26, sample count set 158, idle counters written 158, joint created 322, joint attached 322, ball anchor set 122, rod anchor set 162, fixed joint set 119. Between steps: auto-disable switched 958, body enabled/disabled 1654, force/torque added 4594, body stopped 677, joint parameter set 4886, rod reseated 1687.
- `car`: before the first step: body created 120, mass set 120, position set 120, linear velocity set 19, finite-rotation mode/axis set 120, damping set 120, joint created 420, joint attached 420, rod anchor set 800, fixed joint set 20, joint parameter set 800. Between steps: force/torque added 1695192, joint parameter set 820, rod reseated 200000.
- `strut_car`: before the first step: body created 200, mass set 200, position set 200, linear velocity set 19, finite-rotation mode/axis set 200, damping set 200, joint created 420, joint attached 420, ball anchor set 80, rod anchor set 480, fixed joint set 20, slider axis set 80. Between steps: force/torque added 1694746, joint parameter set 2091, rod reseated 200000.
- `random`: before the first step: body created 4531, mass set 4531, position set 4531, rotation set 4531, linear velocity set 4531, angular velocity set 4531, finite-rotation mode/axis set 4206, damping set 4809, auto-disable switched 524, body enabled/disabled 218, body flags written 390, sample count set 127, inverse mass written 96, idle counters written 127, joint created 4834, joint attached 4834, ball anchor set 876, rod anchor set 6172, rod length set 246, fixed joint set 449, slider axis set 423, joint parameter set 3879, joint flags written 90. Between steps: body created 100, mass set 100, position set 100, linear velocity set 100, finite-rotation mode/axis set 5384, damping set 100, auto-disable switched 1601, body enabled/disabled 2612, force/torque added 1609739, body stopped 8997, body destroyed 87, joint created 100, joint attached 7107, rod anchor set 200, joint parameter set 71214, joint flags written 2335, rod reseated 54329.
<!-- MICRO:END -->

What the worlds contain: masses from 0.5 to 600 kg (the strut car's body about 1,100 kg) as
boxes, as general inertia tensors and as the broken "explicit inertia" matrix; random poses,
half of them given as slightly non-orthonormal matrices;
finite rotation on (as in the game), on with an axis, and off; damping; joints between two
bodies and between a body and the world, attached either way round; several rods between the
same two bodies; closed loops; several islands in one world; ODE's auto-disable with bodies
falling asleep, being woken and disabled by hand, with one or several velocity samples and with
idle times as well as idle step counts; rods of length zero, re-zeroed before every step, in
both of ODE's fallback cases (counted in the notes).

Half of the random worlds are set up exactly as the game sets up its bodies and joints. The
other half also use what the library has code for and the game never does: other gravity
vectors, ERP, CFM, world damping (inherited by new bodies) and step sizes (0.0005 to 0.01 s);
sliders with every limit-motor parameter except the motor force set (the force is given a
value the setter rejects, so the slider stays unpowered; a powered slider is a bounded row),
some of them with stops far away, so that the stop test and the slider position run every
step; a slider axis of length zero; parameter numbers a joint ignores; joints attached to other
bodies or to the world in the middle of a run; the finite-rotation mode and axis switched in
the middle of a run; and, written directly into the object because no linked function can set
them: the "no gravity" flag, a cleared gyroscopic flag, a cap on the angular speed, an inverse
mass of zero (a kinematic body) and the "disabled" flag of a joint. In both halves, chosen by
the seed: a body destroyed on the way (87 of the 1,000 random worlds; the game does that when a
car leaves) and a body with a rod created on the way (100 of them). The lists under the table
say how often each call was made.

A few random worlds blow up to infinities or NaN (noted under the table, with the number of
steps that had a NaN in a body's state or in a constraint force); there a NaN on both sides
counts as equal, because the bits of a NaN cannot
be matched, so those steps prove less than the others. In eleven other random worlds, all with
re-attached joints, the bodies fly apart with numbers that are still finite (in the four that
were looked at they are about a kilometre out at thousands of metres per second) until a slider
passes one of its stops 1,000 m away; ODE then adds a bounded row, which is stage 2, so the
world is ended at that step and its remaining steps are not run: the Rust stepper says "not
implemented" there, the oracle catches it and checks that the game's ODE marked a slider as "at
a stop" in the same step (if it had not, that would count as a difference; in every other step
the stop state of every slider is compared like any other value). That is why the random
worlds have fewer than 1,000,000 steps.

The count of zero-length rods in the notes is made by the oracle with its own arithmetic
before each step, so it is close to, not exactly, the number of times ODE took the fallback.

### 3.2 The maths routines on their own

`tools/ode_oracle matrix` calls single functions of the game directly and compares with the
Rust versions: `_dFactorLDLT`, `_dSolveLDLT`, `_dInvertPDMatrix`, `_dIsPositiveDefinite` and
`_dDot` on random matrices of every size from 1 to 64 rows (40 per size; a quarter of them
arbitrary symmetric matrices, most of which are not positive definite; for the factor and solve
the upper triangle and the padding columns are filled with rubbish, the same on both sides),
and the small rotation and vector functions on 4,000 inputs each, random ones and awkward
ones: zeros of either sign and ones of either sign (which show the order of every sum), ties
for the largest component, lengths from 1e-30 to 1e30, denormal numbers, half-turn rotations
(the three branches of `dQfromR`), the threshold of `dPlaneSpace` to the last bit, rows that
cannot be made orthogonal, and inputs with a NaN or an infinity in them.

<!-- MATRIX:BEGIN -->
| Routines | What is compared | Cases | Values | Cases bit-exact | First difference |
|---|---|---|---|---|---|
| `_dFactorLDLT` + `_dSolveLDLT` | L (strict lower triangle), d, the solution; n = 1 … 64 | 2560 | 1913600 | 2560 | none |
| `_dInvertPDMatrix` | the success flag and the inverse; n = 1 … 64 | 2560 | 3580160 | 2560 | none |
| `_dIsPositiveDefinite` | the answer; n = 1 … 64 | 2560 | 2560 | 2560 | none |
| `_dDot` | the sum; n = 1 … 64 | 2560 | 2560 | 2560 | none |
| `dQMultiply0` | qb * qc; random, signed zeros / ones, NaN, infinity | 4000 | 16000 | 4000 | none |
| `dQMultiply1` | inverse(qb) * qc | 4000 | 16000 | 4000 | none |
| `dQMultiply2` | qb * inverse(qc) | 4000 | 16000 | 4000 | none |
| `dQMultiply3` | inverse(qb) * inverse(qc) | 4000 | 16000 | 4000 | none |
| `dRfromQ` | the 3x3 matrix; unit, half-turn and arbitrary quaternions, NaN, infinity | 4000 | 36000 | 4000 | none |
| `dQfromR` | the quaternion; rotations incl. half turns, matrices a little off, NaN, infinity | 4000 | 16000 | 4000 | none |
| `dDQfromW` | the quaternion rate; also NaN, infinity | 4000 | 16000 | 4000 | none |
| `_dSafeNormalize3` | the flag and the vector; lengths 1e-30 … 1e30, ties, zeros, denormals, NaN, infinity | 4000 | 16000 | 4000 | none |
| `dPlaneSpace` | both vectors; unit normals, the sqrt(1/2) threshold, axes, other lengths, NaN, infinity | 4000 | 24000 | 4000 | none |
| `dOrthogonalizeR` | the 3x3 matrix; near rotations, arbitrary matrices, zero and parallel rows, NaN, infinity | 4000 | 36000 | 4000 | none |
<!-- MATRIX:END -->

Independently of this, the research brief for the solver interpreted the machine code of these
routines instruction by instruction on its own inputs and got the same formulas
(`re/scratch/ode/b_solver.md`).

### 3.3 Replay of the car recordings

`tools/ode_oracle replay` reads the recordings of Task 06 (`oracle/car/*.carrec`, the game's own
F2004) and builds the same six bodies and 21 joints in Rust.

- **Per step:** for every step N the recorded state at the start of `dWorldStep` is loaded, the
  recorded accumulators are set, the joints get their recorded parameters, Rust steps once, and
  the result is compared with the recorded state after the step: position, quaternion, rotation
  matrix, linear and angular velocity and place in the island (`tag`) of every body; constraint
  force (on both bodies) and place in the island's joint order (`tag`) of every joint.
- **Free run:** only the state at the start of step 0 is loaded. Then the whole scenario runs
  in Rust, through the spawn drop, and the Rust state is never reset. Each step gets what the
  car did to ODE from outside: the calls of the force tape, one by one in recorded order,
  through the Rust add-force functions (so the accumulators are rebuilt from the Rust state's
  own pose; the sleeping rule's "stop" is on the tape too), the joints' parameters (the steering
  moves two rod anchors, the game switches the joints' ERP below 1 m/s) and the bodies' masses
  (fuel burn). After every step the same comparison as above is made, and before it the
  rebuilt accumulators are compared with the recorded ones.

<!-- REPLAY:BEGIN -->
| Scenario | Steps | Steps with contact joints (stage 2) | Per step: bit-exact among the contact-free steps | Free run: first difference | Free run: accumulators rebuilt from the tape, bit-exact steps |
|---|---|---|---|---|---|
| `brake` | 5334 | 0 | 100 % (5334/5334) | none | 100 % |
| `kerb` | 4001 | 0 | 100 % (4001/4001) | none | 100 % |
| `launch_autoclutch_off` | 4667 | 0 | 100 % (4667/4667) | none | 100 % |
| `launch_autoclutch_on` | 4667 | 0 | 100 % (4667/4667) | none | 100 % |
| `liftoff_oversteer` | 5334 | 0 | 100 % (5334/5334) | none | 100 % |
| `random` | 20001 | 0 | 100 % (20001/20001) | none | 100 % |
| `settle` | 667 | 0 | 100 % (667/667) | none | 100 % |
| `settle_floor` | 667 | 12 | 100 % (655/655) | step 53, the first step with floor contact joints | 8.0960 % (54/667) (the tape's forces no longer fit the Rust state after the contact) |
| `slalom` | 5334 | 0 | 100 % (5334/5334) | none | 100 % |
| `steady_corner_120` | 6667 | 0 | 100 % (6667/6667) | none | 100 % |
| `steady_corner_180` | 6667 | 0 | 100 % (6667/6667) | none | 100 % |
| `steady_corner_60` | 6667 | 0 | 100 % (6667/6667) | none | 100 % |
| **all** | **70673** | **12** | **100 % (70661/70661)** | | |
<!-- REPLAY:END -->

<!-- TAPE:BEGIN -->
Calls on the force tapes of the twelve recordings, all fed through the Rust functions by the free runs: `addForceAtPos` 1970040, `addForceAtLocalPos` 565384, `addLocalForceAtLocalPos` 1757978, `addTorque` 282692, `stop` 10444. Never called in these recordings: `addLocalForce`, `addLocalForceAtPos`, `addLocalTorque`, `setVelocity`, `setAngularVelocity`, `setPosition`, `setRotation`.
<!-- TAPE:END -->

So the free runs prove four of the seven add-force functions on real driving
(`dBodyAddForceAtPos`, `dBodyAddForceAtRelPos`, `dBodyAddRelForceAtRelPos`, `dBodyAddTorque`).
The other three (`dBodyAddForce`, `dBodyAddRelTorque`, `dBodyAddRelForceAtPos`) never occur in
these recordings and are covered by the micro-oracle only, as are ball and slider joints,
joints to the world, auto-disable and `dBodyDestroy`.

The recordings of Task 06 were made with a feedback buffer on every joint (that is how the
constraint forces got into them); the game itself runs without. To see that this changes
nothing, two more recordings were made for this task without the buffers
(`car_oracle run --scenario slalom --no-joint-forces`, the same for `settle_floor`; kept in the
git-ignored `re/scratch/ode/nojf/`) and replayed with the Rust joints' feedback off:

<!-- NOJF:BEGIN -->
| Scenario | Steps | Steps with contact joints (stage 2) | Per step: bit-exact among the contact-free steps | Free run: first difference | Free run: accumulators rebuilt from the tape, bit-exact steps |
|---|---|---|---|---|---|
| `slalom` | 5334 | 0 | 100 % (5334/5334) | none | 100 % |
| `settle_floor` | 667 | 12 | 100 % (655/655) | step 53, the first step with floor contact joints | 8.0960 % (54/667) (the tape's forces no longer fit the Rust state after the contact) |
| **all** | **6001** | **12** | **100 % (5989/5989)** | | |
<!-- NOJF:END -->

`settle_floor` is the one recording made with a collision mesh under the car: in 12 of its
steps (53 to 64) the game's world also held four contact joints between the car's floor and the
road. Contacts are stage 2, so those steps are left out of the per-step count, and the free run
of that scenario leaves the recording at step 53, as it must: the Rust world has no floor. From
then on the taped forces belong to a car that is somewhere else, so that free run is no longer
a simulation of anything (the bodies end 0.38 m from the recording after 2 s) and its later
steps say nothing. All other steps of that recording, before and after the contact, are
bit-exact in the per-step test.

### 3.4 Golden tests in `cargo test` (no game needed)

Three test files in `crates/rustyac-ode/tests/` hold the game's answers and pass:

- `golden_car.rs` replays two excerpts of the car recordings without ever resetting the state
  (the recorded accumulators are loaded before each step; the add-force functions are not
  called here, `golden_ops.rs` does that): the first 300 steps
  of `settle` (the spawn drop, the settling, the freeze by the sleeping rule from step 155) and
  steps 3000 to 3299 of `slalom` (100 km/h, both steering rods moving every step). The excerpts
  (`tests/data/*.odegold`, 168 kB each, written by `ode_oracle excerpt`) hold the start state
  and, per step, the accumulators, changed joint parameters and masses, the stop mask, the
  expected position, quaternion and velocities of the six bodies and a hash of the 21 joints'
  constraint forces.
- `golden_ops.rs` replays 31 small worlds of the micro-oracle (`tests/data/ops/*.odeops`,
  507 kB, written by `ode_oracle golden`): each file is the list of calls that built the world
  and the calls before every step, with a hash of the state of the game's ODE after every step
  (6,420 steps in all). They cover what the car excerpts do not: ball and slider joints, joints
  to the world, rods of length zero, auto-disable, the F2004 layout and the strut layout driven
  by forces, a destroyed body, a body created on the way, re-attached and disabled joints,
  other gravity, damping and step sizes, and worlds without joint feedback buffers.
- `golden_functions.rs` checks the maths routines one by one against 2,246 cases of section 3.2
  with the game's answers (`tests/data/functions.odefunc`, 234 kB, written by
  `ode_oracle matrix`).

All three also pass with `RUSTYAC_MATH=std` set, that is with Rust's own sine and cosine in
place of the game's C runtime (see 2.1).

As a check that these tests can fail, two faults were put into the crate by hand and removed
again: a wrong factor in one slider row (caught in step 0 of the first world with a slider) and
an auto-disable counter off by one (caught in step 19 of the first auto-disable world).

---

## 4. Performance

`tools/ode_oracle bench`: the F2004 layout of the micro-oracle (6 bodies, 21 joints, 26 rows,
no joint feedback buffers, as in the game), 20,000 steps, both engines given the same forces,
only the step call timed, best of five rounds; the command was run three times on this PC:

<!-- BENCH:BEGIN -->
| Run | The game's `dWorldStep` (in `acs.exe`) | `rustyac-ode` `World::step` | Ratio |
|---|---|---|---|
| 1 | 7.98 µs | 9.25 µs | 1.16 |
| 2 | 8.11 µs | 9.32 µs | 1.15 |
| 3 | 8.07 µs | 9.29 µs | 1.15 |
<!-- BENCH:END -->

Rough numbers: the timer is around a call of less than ten microseconds, the two engines are
stepped alternately, and the result moves with what else the PC is doing. With feedback buffers
on every joint (`bench --feedback`) four runs gave 7.9 against 10.4, 8.0 against 10.4, 7.9
against 8.8 and 8.1 against 9.2 µs. Runs made earlier in this task gave about 23 against 25 µs for the same scene
and the same stepped code; the PC was presumably busier then (not investigated). Over all runs
the Rust step took between 1.09 and 1.31 times as long as the game's. No attempt was made to
make the Rust code fast (all indexing is bounds-checked, the scratch arrays are cleared and
refilled every step); the order of the float operations is fixed, so speed can only come from
the code around them.

---

## 5. Commands

```
cargo test -p rustyac-ode                          # the golden tests (no game needed)
cargo test -p rustyac-math -p rustyac-physics      # the earlier ports, after the maths module moved

cargo build --release --manifest-path tools/ode_oracle/Cargo.toml
ode_oracle micro                                   # every world type, about a minute; writes oracle/ode/micro_results.md
ode_oracle micro --type car,random --worlds 50 --steps 500 --first-seed 1000 --verbose   # writes micro_partial.md
ode_oracle micro --type dball --worlds 5 --record re/scratch/ode/states      # also store the game's states
ode_oracle matrix                                  # single functions; writes oracle/ode/matrix_results.md and tests/data/functions.odefunc
ode_oracle replay                                  # all recordings in oracle/car; writes oracle/ode/replay_results.md
ode_oracle replay oracle/car/slalom.carrec --verbose                          # writes replay_partial.md
ode_oracle excerpt                                 # rewrite crates/rustyac-ode/tests/data/*.odegold from the recordings
ode_oracle golden                                  # rewrite crates/rustyac-ode/tests/data/ops/*.odeops from the game
ode_oracle bench [--steps 20000] [--feedback]
```

(Run from the repository folder; `ode_oracle` stands for
`tools/ode_oracle/target/release/ode_oracle.exe`. `micro`, `golden`, `matrix` and `bench` need
the game's `acs.exe` (`--acs <path>`, default the Steam folder) and the Visual C++ 2013 runtime;
`replay` and `excerpt` need only the recordings, which `car_oracle all` makes. `micro`,
`golden`, `matrix` and `replay` end with an error if something differs; `micro` and `replay`
also if no step was compared, `micro` if a world type is unknown; `bench` and `excerpt` compare
nothing. Only the full `micro` and `replay` runs write `micro_results.md` and
`replay_results.md`; the table of the two recordings without buffers in 3.3 comes from
`replay_partial.md` of a run with those two files named. World types for `--type`: `free_body`, `ball`,
`dball`, `dball_zero`, `fixed`, `slider`, `auto_disable`, `car`, `strut_car`, `random`. With the
environment variable `ODE_ORACLE_DETAIL` set, `micro` prints the first 60 differing values of the
first differing step and the world's set-up.)

Reading unnamed functions: `python tools/pdata.py range 0x14034f400 0x1403526a0` lists the
function chunks, `python tools/disasm.py range <lo> <hi>` disassembles one.

---

## 6. Open questions

### 6.1 Choices made while working unattended

1. **The maths module moved to a new crate `rustyac-math`** and `rustyac-physics` re-exports it
   as `rustyac_physics::math`, so nothing else had to change. `rustyac-ode` must not depend on
   `rustyac-physics` (the car code will depend on the rigid bodies, not the other way round).
2. **A new tool, `tools/ode_oracle`**, not a mode of `car_oracle`. It reuses three source files
   of `car_oracle` by path (the `acs.exe` loader, the recording reader, the call-site table).
3. **"Feeding only the taped forces" was read as "feeding only what the car does to ODE from
   outside"**: in the free run that is the force tape plus joint parameters and body masses,
   which the steering, the game's softness rule and the fuel change between steps. State is
   never fed.
4. **Contact steps are outside stage 1**: `settle_floor`'s 12 contact steps are reported, not
   counted as failures.
5. **Bounded rows stop the program** (`unimplemented!`) instead of being solved wrongly: a
   slider that has reached a stop or has a motor force, or anything that is not a full equality
   joint.
6. **`dBodyDestroy` was ported** (the game calls it when a car leaves) and tested; destroying a
   world is just dropping the value. A destroyed body keeps its slot (`Body::alive` is false,
   `World::body_ids` skips it); using its handle afterwards is the caller's mistake, as in ODE.
7. **Joint feedback** is a switch on the Rust joint (`joint_set_feedback`), since there is no
   pointer to hand over. The game never asks for it; the recordings do.
8. **Body state is public** (`Body::pos`, `q`, `r`, `lvel` …), so a caller can set the
   quaternion directly; the game's ODE has no `dBodySetQuaternion` linked either, its wrapper
   uses `dBodySetRotation`.
9. **NaN**: two NaN values count as equal in every comparison; where the machine code treats NaN
   differently from C the port follows the machine code.
10. **The golden files are 1.1 MB** of binary test data in git (car excerpts 337 kB, synthetic
    worlds 507 kB, function cases 234 kB). They could be shorter; the sizes were chosen so that
    the spawn drop, the freeze and full steering are inside the car excerpts and every joint
    type and every unusual call is inside the synthetic worlds.
11. **ODE's licence text** was taken from the official repository at tag 0.13.1, because the
    release archive does not contain `LICENSE-BSD.TXT` although its `COPYING` refers to it.
12. **Subagents**: five read-only readers (three at a time) compared machine code and source
    and wrote the briefs; three read-only reviewers then went through the finished crate, the
    oracle and this report looking for mistakes (their list: `re/scratch/ode/review_findings.txt`).
    None edited the crate or committed.
13. **`docs/map/car.md` and `docs/map/physics_engine.md`** got short notes saying which of
    their open questions this task answered.
14. **What the review changed.** The reviewers found no case in which the port gives different
    bits from the game for finite numbers. They found two NaN-only differences in the crate
    (a NaN inverse mass in the gyroscopic test and in `dxJoint::isEnabled`; fixed) and one gap
    (a slider with stops set went straight to "not implemented" although ODE only adds the
    bounded row when a stop is reached; the stop test and `dJointGetSliderPosition` are ported
    now), one linked function that was missing (`dBodySetAutoDisableAverageSamplesCount`,
    ported now), and a list of things the oracle did not exercise or did not compare. All of those
    were added: worlds without joint feedback buffers, the feedback sentinel, joint flags and
    bodies in the comparison, re-attached joints, bodies created on the way, kinematic bodies,
    disabled joints, slider parameters, other world settings, the sample count of auto-disable,
    rods re-zeroed every step, the single-function checks of 3.2, and the game-free golden
    tests of 3.4. Every new case matched at the first run; none needed a correction of
    arithmetic that was already in the crate. A last read-only pass by two fact checkers
    compared this report sentence by sentence with the crate and the oracle
    (`re/scratch/ode/factcheck_findings.txt`); its 26 corrections of wording are in the text,
    and three of them became checks: the slider's stop state is compared in every step, an
    early end is checked against the game, and the single-function checks got NaN and infinity
    inputs for every function.
15. **The oracle writes into objects directly** where the game's library has code but no linked
    function to reach it (body flags, inverse mass, idle counters, the joint "disabled" flag, the
    feedback pointer). That tests more of the stepper than the game can reach; it is marked as
    such in the tool.

### 6.2 Questions

1. **Floating-point mode of the real game's physics thread.** Everything here ran in the
   oracle's process with the default rounding and denormal handling. `acs.exe` itself never
   changes the SSE control register in its physics code (one brief checked: no `ldmxcsr`, and
   `_controlfp` only in two post-processing functions), but a library loaded by the real game
   could. If the real game ran with "denormals are zero", values below about 1e-38 would differ.
   Not measurable without the running game.
2. **Other suspension types.** The strut layout of the micro-oracle is built the way
   `physics_engine.md` describes it (slider strut–hub, ball body–strut, three rods), but the
   creation order inside `SuspensionStrut` and `SuspensionAxle` was not read. It decides the
   row order, so it has to be read (or recorded with a strut car in `car_oracle`) when those
   suspensions are ported.
3. **Speed.** Is "about 15 % slower than the original" enough for now, or should the stepper
   be tuned before more is built on it?
4. **Stage 2 starts from a known trap**: the solver's `nub` argument (2.4) and the row order
   with bounded rows (unbounded joints reversed, then bounded ones in island order) are written
   down, but the pivoting code itself has not been read at instruction level yet, and it
   contains exactly the kind of loops the compiler turned into packed sums.
5. **`settle_floor`** can serve as the first golden for stage 2 once contact points can be fed
   in, but the recording holds only the number of contact joints, not their points, normals and
   depths; `car_oracle` would have to record them.
6. **"Pure Rust" and the C runtime.** The task asked for a crate in pure Rust without
   dependencies and for the maths module of `rustyac-physics` to be reused. Those two pull
   apart a little: that module (now `rustyac-math`) calls the game's `MSVCR120.dll` for sine
   and cosine when the DLL is installed, so that the bits are the game's by construction, and
   only otherwise uses Rust's own. The golden tests pass both ways on this PC. Should
   `rustyac-ode` keep that, or always use Rust's own functions?
7. **Sliders that fly past a far stop** end eleven random worlds early (3.1). Nothing is wrong
   there, but those worlds will only run to the end once stage 2 has bounded rows; the oracle
   will then stop ending them by itself, because the Rust stepper will no longer refuse.
