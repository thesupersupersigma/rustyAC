# Assetto Corsa tyre code map (acs.exe, read-only)

Source: `acs.exe` + `acs.pdb` in the Ghidra 12.1.2 project `ghidra/acs.gpr`, read through the
kawaiidra MCP (real PDB names confirmed, e.g. `Tyre::addTyreForcesV10` @ `0x14027ed60`).
Raw pseudo-C: `re/tyre/decomp/`. Struct layouts: `re/tyre/types/`. ini mapping: `re/tyre/ini_keys.md`.
Nothing in the Ghidra project, the game folder or git was changed. No Rust was written.

Function count: the MCP name search finds **66 functions whose own name contains "tyre"**
(it matches the bare function name, not the class). On top of those, the tyre classes have many
methods without "tyre" in the name (`Tyre::step`, `SCTM::solve`, …) which were found by following
calls and the unwind table. 84 functions are saved in `re/tyre/decomp/` (index: `re/tyre/functions.tsv`).

---

## 1. Plain-English summary

Each physics step (333 times a second) every wheel of every car runs the same routine.
First the wheel asks the track "what is directly underneath me?" by dropping one straight line
down from just above the wheel centre; the answer is one point on the ground, the direction the
ground faces there, and what kind of surface it is (grip, bumpiness, dirt).
From how far the wheel has sunk into that point, the tyre works out how hard it is being pressed
into the ground, treating the tyre as a single spring with a damper; that push goes straight
into the suspension.
Next it compares how fast the tyre's surface is moving with how fast the ground is passing
underneath, forwards and sideways, to get how much the tyre is slipping in each direction.
That slip, the load, the lean of the wheel, the tyre's temperature, pressure, wear and the grip
of the surface are fed into a formula that returns the forward/backward grip force, the sideways
grip force and the self-centring twist felt in the steering.
Those forces are pushed into the suspension at the contact point, and the forward force is also
turned into a torque that speeds the wheel up or slows it down, together with brakes and rolling drag.
Finally the tyre updates its slow-changing state: it heats up from sliding and rolling and cools
from air and the road, its pressure follows its temperature, and it accumulates wear, graining,
blistering, flat spots and dirt, all of which change the grip on later steps.
Cars built for tyre format version 10 or newer use a newer, more complete formula; older cars
use the original one.

---

## 2. Classes

### Physics

| Class | What it is for |
|---|---|
| `Tyre` (0x858 bytes) | One wheel's tyre. Owns everything below and does the per-step work: ground contact, forces, wheel spin, thermal/wear stepping. |
| `TyreModelData` (0x290) | Grip-model parameters of the current compound (version, load sensitivity, camber, rolling resistance, pressure gains, relaxation length, curves). Physics data. |
| `TyreData` (0x48) | Carcass/geometry numbers of the current compound (radius, rim radius, spring rate `k`, damping `d`, angular inertia, thermal and grain/blister constants). Physics data. |
| `TyreInputs` (0xc) | Torques applied to the wheel this step: brake, handbrake, electric. Physics input. |
| `TyreExternalInputs` (0x10) | Override of load / slip angle / slip ratio (used by the tyre test bench). Physics input. |
| `TyreStatus` (0xb8) | Live state and outputs: load, Fx, Fy, Mz, slip angle/ratio, radii, angular velocity, pressure, wear (virtualKM), grain, blister, flat spot, dirt. Physics state. |
| `TyreSlipInput` / `TyreSlipOutput` | Argument/return of the old brush slip provider (`BrushSlipProvider::getSlipForce`). Physics. |
| `TyreModelInput` / `TyreModelOutput` | Argument/return of `ITyreModel::solve` (the V10 model). Physics. |
| `ITyreModel` → `SCTM` (0x1e8) | The V10 force formula ("SCTM"). `Tyre::tyreModel` always points at `Tyre::scTM`. Physics. |
| `BrushSlipProvider` (0x38) + `BrushTyreModel` / `BrushTyreModelData` | The older slip→friction curve (brush model) used by the pre-V10 path and to pre-compute the peak. Physics. |
| `TyreThermalModel` (0xe0) | Tyre temperatures: 12 × 3 surface patches around the tyre plus a core; gives the grip multiplier from temperature. Method `buildTyre` creates and links the patches. Physics. |
| `TyreThermalPatch` (0x28) | One surface patch: temperature `T`, pending heat `inputT`, neighbours. Physics. |
| `TyrePatchData` (0x14) | Heat-transfer coefficients of the thermal model. Physics data. |
| `TyreCompoundDef` (0x3f0) | One compound as loaded from tyres.ini (name + `TyreModelData` + `TyreData` + slip provider + thermal data). Loaded data, copied into `Tyre` by `setCompound`. |
| `TyreTester` (0x128) | Offline test bench: owns a standalone `Tyre` and runs slip-angle / slip-ratio sweeps through `Tyre::step`. Uses the physics, is not part of the car step. |

### Not physics (listed only, not analysed)

| Item | What it is |
|---|---|
| `TyreThermalState` (0xc0) | Snapshot of temperatures copied out for display/telemetry (`Car::getTyreThermalState`). |
| `TyreDeflection` (0xe0) | No physics code found using it; graphics-side data. |
| `TyreCompoundStrategy` (0x4) | AI pit strategy entry (`initTyreStrategies`, `findTyreForQualify`). |
| `TyreTesterForm`, `TyresApp`, `TyreWearingDebug`, `renderTyreTemp`, `SetupTyresManager` | UI / debug apps. |
| `TyreBlur`, `TyreMaterials`, `initTyreMaterials`, `TyreSmoke`, `makeTyresDoubleFacedShadows`, `getTyreMatrix` (0x140108590) | Rendering. |
| `AIDriver::updateTyres`, `stepSplineSelection`, `findTyreForQualify`, `initTyreStrategies` | AI. |
| `onTyreCompoundChanged`, `getTyreCompound`, `getTyreCompoundIndex`, `setTyreCompound`, `ac_getCarTyreCompound`, `getPitStopTyrePercentage`, `isChangingTyres`, `setAllowedTyresOut`, `addLegalTyre`, `isTyreLegal` | Game rules / pit stop / scripting glue. |
| `RaceEngineer::evalTyreLoad`, `getDrivingTyresSlip`, `findTyreCompound`, `getLeftDrivenTyre`, `getRightDrivenTyre` | Helper estimates for AI/setup (the first three are saved in `decomp/` because they do tyre maths). |
| `std::vector<TyreCompoundDef>` / `std::vector<TyreThermalPatch>` helpers, `_Uninit_move<…>` | Compiler-generated container code. |

---

## 3. Call chain: physics update → tyre forces

```
std::_LaunchPad<…>::_Go                       0x140123030   physics thread entry
└─ PhysicsDriveThread::run                    0x140123150   (also PhysicsDriveThread::step 0x140123560)
   └─ PhysicsEngine::step(dt = 0.003, …)      0x140264760   call sites 0x1401232df / 0x1401236bf
      ├─ Car::stepPreCacheValues (each car)
      └─ Car::step(dt)                        0x140275da0   call sites 0x1402648f9, 0x140264b0e
         │                                                  (or via ThreadPool task → _Do_call 0x140263080)
         ├─ Car::stepThermalObjects
         └─ Car::stepComponents(dt)           0x1402764d0   call site 0x140276475
            ├─ BrakeSystem::step, EDL::step, each suspension->step
            └─ Tyre::step(dt)  (for each of the tyres)      0x140283800   call site 0x140276579
               ├─ hub->(vtable+0x08)  wheel world matrix
               ├─ rayCaster / rayCollisionProvider ray cast (one ray, straight down)
               ├─ Tyre::addGroundContact      0x14027d980   load from spring+damper, pushed into hub
               ├─ if modelData.version < 10:
               │     Tyre::addTyreForces      0x14027e1a0   call site 0x140283f9b
               │     ├─ Tyre::stepRelaxationLength 0x140284a40
               │     ├─ Tyre::getDY 0x1402803d0 / getDX 0x140280240 / getCamberedDy 0x140280080
               │     ├─ Tyre::getCorrectedD   0x140280190 → TyreThermalModel::getCorrectedD 0x1402adbc0
               │     ├─ slipProvider vtable[0] → BrushSlipProvider::getSlipForce 0x1402b3190
               │     │     └─ BrushTyreModel::solve 0x1402cb3c0 / solveV5 0x1402cb4e0
               │     ├─ Tyre::stepDirtyLevel 0x1402843d0, Tyre::stepPuncture 0x1402849b0
               │     └─ hub->(vtable+0x18)  force at (contact point + trail)
               │  else:
               │     Tyre::addTyreForcesV10   0x14027ed60   call site 0x140283f94
               │     ├─ Tyre::getCorrectedD
               │     ├─ tyreModel vtable[+8] → SCTM::solve 0x14044bc20
               │     │     ├─ SCTM::getStaticDY 0x14044bbb0 / getStaticDX 0x14044bb30
               │     │     └─ SCTM::getPureFY 0x14044ba70 (twice: X and Y)
               │     ├─ Tyre::stepDirtyLevel, Tyre::stepPuncture
               │     ├─ hub->(vtable+0x18) force at contact point   (torqueModeEx == original)
               │     │   or Tyre::addTyreForceToHub 0x14027dc00 → hub->(vtable+0xc0)
               │     └─ hub->(vtable+0x20) aligning torque Mz about the ground normal
               ├─ feedback torque → Tyre::updateAngularSpeed 0x140284fa0 (+ updateLockedState 0x140285070)
               │                    and Tyre::stepRotationMatrix 0x140284b80   (non-driven wheels;
               │                    driven wheels get their speed from the Drivetrain)
               ├─ Tyre::stepThermalModel      0x140284ca0
               │     ├─ TyreThermalModel::addThermalInput 0x1402ad090 / addThermalCoreInput 0x1402ad070
               │     ├─ TyreThermalModel::step 0x1402ae340
               │     └─ Tyre::stepTyreBlankets 0x140284f10
               ├─ Tyre::stepGrainBlister 0x140284600, Tyre::stepFlatSpot 0x140284500
               └─ onStepCompleted callback
```

Other callers of `Tyre::step`: `TyreTester::slipAngleSweep` 0x14044f300, `slipRatioSweep`
0x14044f710, `srSASweep` 0x14044fb00 (test bench, not the car).

The timestep literal `0.003` is passed by `PhysicsDriveThread` to `PhysicsEngine::step`.

---

## 4. Data flow: tyres.ini → structs → force functions → hub

1. **Load** — `Car::Car` (0x14026bf00) → `Tyre::init` (0x140280650) → `Tyre::initCompounds`
   (0x140280800). Reads `tyres.ini` with `INIReader`, builds one `TyreCompoundDef` per
   `[FRONT]`/`[REAR]`(`_n`) section (plus `[THERMAL_FRONT]`/`[THERMAL_REAR]`), stores them in
   `Tyre::compoundDefs`. Full key table: `re/tyre/ini_keys.md`.
2. **Select** — `Tyre::setCompound(i)` (0x1402834e0) copies def *i* into the live objects:
   `Tyre::modelData` (TyreModelData), `Tyre::data` (TyreData), `Tyre::slipProvider`
   (BrushSlipProvider), `Tyre::thermalModel.patchData` + `.performanceCurve`,
   `Tyre::status.pressureStatic`, and mirrors the V10 parameters into `Tyre::scTM` (SCTM):
   `asy`, `falloffSpeed`, `Fz0`, `lsExpX/Y`, `lsMultX/Y`, `maxSlip0/1`, `speedSensitivity`,
   `camberGain`, `dcamber0/1`, `dyLoadCurve`, `dxLoadCurve`, `pressureCfGain`, `cfXmult`,
   `brakeDXMod`, `dCamberCurve`, `combinedFactor`.
3. **Per step inputs** — `Tyre::inputs` (brake / handbrake / electric torque, written by other
   car systems), hub matrix and velocities from `ISuspension` (`Tyre::hub`), the ray-cast hit
   (point, normal, `SurfaceDef`), `Tyre::status` from the previous step.
4. **Vertical** — `addGroundContact`: `depth = liveRadius - distToGround`;
   `k = data.k + (pressureDynamic - pressureRef) * pressureSpringGain` (200000 once the rim touches);
   `load = depth * k - data.d * (hubVelocity · normal)`; force `load * normal` applied to the hub
   at the contact point; `status.load` clamped ≥ 0.
5. **Slip** — road-aligned axes (`roadHeading`, `roadRight`) are the wheel axes projected onto
   the ground plane; contact-point velocity is projected onto them; slip angle via
   `ksCalcSlipAngleRAD` (`atan(-vy/|vx|)`), slip ratio from `ω·effectiveRadius` vs road speed;
   camber via `ksCalcCamberRAD`.
6. **Grip level** — `Tyre::getCorrectedD` = thermal multiplier (`thermalModel.thermalMultD`, from
   `performanceCurve` at the practical temperature) ÷ pressure penalty × wear curve at
   `status.virtualKM`; then × `SurfaceDef::gripMod` × track `dynamicGripLevel`.
7. **Force formula** — V10: `SCTM::solve(TyreModelInput{load, slipAngleRAD, slipRatio, camberRAD,
   speed, u, tyreIndex, cpLength, grain, blister, pressureRatio, useSimpleModel})` →
   `TyreModelOutput{Fy, Fx, Mz, trail, ndSlip, Dy, Dx}`. Older: brush slip curve × load × D.
8. **Out** — `status.Fx/Fy/Mz/Dx/Dy/ndSlip` are stored; the world force
   `Fy·roadRight + Fx·roadHeading` goes to the hub (`ISuspension` vtable +0x18, or
   `addTyreForceToHub` → +0xc0); Mz goes to the hub as a torque about the ground normal
   (+0x20); rolling resistance is stored in `status.rollingResistence`.
9. **Wheel spin** — `status.feedbackTorque` = tyre reaction + rolling resistance − brake torque
   (`max(brake·absOverride, handbrake)`) + electric torque; non-driven wheels integrate it in
   `updateAngularSpeed` (`ω += torque / angularInertia · dt`, with lock detection); driven wheels
   are handled by `Drivetrain`.
10. **Slow state** — `stepThermalModel` (heat in = slide speed × D × load × `thermalFrictionK`
    × grip, plus rolling terms), `pressureDynamic = (coreTemp − 26) · pressureTemperatureGain +
    pressureStatic`, `stepGrainBlister`, `stepFlatSpot`, `stepDirtyLevel`, `stepPuncture`,
    `virtualKM` accumulation.

---

## 5. V10 vs the older path

Selection is in `Tyre::step`: `if (modelData.version < 10) addTyreForces(...) else
addTyreForcesV10(...)`. `modelData.version` is `[HEADER] VERSION` of the car's tyres.ini
(read in `initCompounds`, stored at `TyreModelData+0x0`). The same test is used later in
`Tyre::step` to pick the wheel-torque formula.

The old path does not go through `ITyreModel`. `Tyre::tyreModel` points at the SCTM for every
version, but `addTyreForces` works out the force itself and takes only the slip curve from
`Tyre::slipProvider` (`BrushSlipProvider::getSlipForce`, the one virtual of that class, vtable
0x1404f8258). Its single `tyreModel->solve` call has no effect (section 7, item 3).

| | `addTyreForces` (VERSION < 10) | `addTyreForcesV10` (VERSION ≥ 10) |
|---|---|---|
| Force formula | `BrushSlipProvider::getSlipForce` → `BrushTyreModel::solve` (`slipProvider.version < 5`) or `solveV5`; gives a share of the peak force and the normalised slip, then `F = load · share · D` split along the sliding direction. `getSlipForce` reads only the slip and the load of its `TyreSlipInput`. Below `VERSION` 5 it calls `solve` with a fall-off level of 1.0, so the force never falls after the peak. From 5 on it passes `asy` (`FALLOFF_LEVEL`) to `solveV5`, or 1.0 when `1.0 >= aiMult` is false | `ITyreModel::solve` → `SCTM::solve`; returns Fx, Fy, Mz, trail, ndSlip, Dx, Dy directly |
| Slip measure | Sliding **velocities** (`rSlidingVelocityX/Y`), combined as a vector: slip = slide speed ÷ hub speed (the slide speed itself when the hub speed is not above 1 m/s). The slip ratio does not enter the grip force; `status.slipRatio` is `−(slidingVelocityX / roadVelocityX)`, divided by the signed road speed (V10 divides by its absolute value) and not relaxed. Here `slidingVelocityX = ω·effectiveRadius + roadVelocityX`: `roadVelocityX` has the opposite sign to the V10 path and the hub's own angular velocity is not added. `status.slipAngleRAD` is the raw `ksCalcSlipAngleRAD` value | Slip **ratio** and slip **angle**, combined with a square-root sum or the `COMBINED_FACTOR` power norm |
| Relaxation (lag) | `stepRelaxationLength` low-pass filters the sliding velocities. Its speed argument (PDB name `hubVelocity`) is the wheel's surface speed `|ω|·effectiveRadius`, not the hub speed, and its load term divides by `TyreModelData::Fz0`, which is always 2000 | The same lag formula is applied inline to `status.slipRatio` and `status.slipAngleRAD`; below 1 m/s a clamped low-speed substitute is used |
| Peak grip D | `Tyre::getDY/getDX` (linear `Dy0+Dy1·load`, exponent, or curve), camber via `getCamberedDy`, then `getCorrectedD`, then `÷ (1 + speedSensitivity·slideSpeed)` | `SCTM::getStaticDY/DX` (exponent or curve), camber D-loss, `÷ (1 + speedSensitivity·slideSpeed)`, all inside `solve`; `getCorrectedD` enters as input `u` |
| Camber thrust | `camberGain · sin(camber) · roadVelocityX` added to the lateral sliding velocity | `camberGain · sin(camber)` added to the slip angle |
| Extra V10-only effects | — | `brakeDXMod` (less longitudinal grip under braking), `cfXmult` (separate longitudinal stiffness), pressure → cornering stiffness (`pressureCfGain`), load-dependent peak slip (`maxSlip0/1`), blister reduces D, grain softens stiffness, `radiusRaiseK` tyre growth read from ini |
| Aligning torque | Not applied as a torque: the force is applied at `contact point + trail · roadHeading`; `status.Mz = −trail·Fy` is only recorded | `Mz` from the model is applied to the hub as a real torque about the ground normal |
| Force into hub | Always `hub vtable+0x18` (force at point) | `torqueModeEx == original`: same call at the contact point; otherwise `addTyreForceToHub` (splits into hub-local force and torque) |
| Wheel torque from tyre | `loadedRadius · Fx` | `−localMX` (set in the V10 path; equals `loadedRadius·Fx` in the original torque mode) |
| Loose surfaces | Special case when `SurfaceDef::granularity != 0` (`1 − exp(−14·slip)`, D = 0.65); it also writes `gripMod = 1.0` into the `SurfaceDef` itself, where it stays for every later step and tyre | No special case in this function |
| Wear distance | `totalSlideVelocity · dt · tyreConsumptionRate` | slide speed `· dt · tyreConsumptionRate`, optionally `× load/Fz0` (`[VIRTUALKM] USE_LOAD`) |
| `status.D` | corrected D used for the force | `SCTM::getStaticDY(load)` |

Shared by both: road axes construction, `stepDirtyLevel`, `stepPuncture`, the rolling-resistance
block (`rr0`, `rr1`, pressure gain, `rr_slip` or `rr_sa/rr_sr` for version 1), NaN guards.

Separate, older switch inside the loader: `VERSION < 5` reads `DY0/DY1/DX0/DX1` (linear load
sensitivity) and uses `BrushTyreModel::solve`; `VERSION ≥ 5` reads `FZ0/LS_EXP*/D*_REF` and sets
`slipProvider.version = 5` (`solveV5`). `XMU` is used below 5 only. The provider's constructor
`BrushSlipProvider(maxAngle, xu, flex)` 0x1402b2f80 does not store its `xu` argument; the loader
writes `brushModel.data.xu` afterwards, in the `VERSION < 5` branch. The fall-off level `asy` is
0.85 below 5 and 0.92 from 5 on. From `VERSION` 7 on `FALLOFF_LEVEL` and `FALLOFF_SPEED` replace
it, after `maximum` / `maxSlip` were computed. The only stock data below `VERSION` 10 is in the
game's sdk: the kart `formula_k` and 100 tyre sets with `VERSION` 7, 30 tyre sets with `VERSION` 3.

---

## 6. Single contact point?

**Yes — confirmed.** `Tyre::step` casts exactly one ray per wheel per step: origin = wheel
centre + 2 m up (world Y), direction `(0, −1, 0)`, through `IRayCaster` (or
`IRayTrackCollisionProvider` when there is no caster). The single hit gives one point, one
normal and one `SurfaceDef`. Details:

- The hit is ignored (tyre treated as airborne: `ndSlip = 0`, `Fy = 0`) unless the wheel's up
  axis has world-Y component > 0.35.
- The contact point is the wheel centre projected onto the hit plane; if the hit normal is more
  than `acos(0.96)` (~16°) away from the wheel's up axis, the normal is rotated back toward it
  and the raw hit point is used.
- Surface bumps are added procedurally to that one point's height (`sinHeight`/`sinLength`, and
  a three-sine pattern when `granularity != 0`).
- Load comes from one spring-damper along the normal (`addGroundContact`); there is no patch
  integration, no multiple rays, no width sampling.
- A contact-patch **length** is computed analytically (`ksCalcContactPatchLength(liveRadius,
  depth) = 2·sqrt(r² − (r−depth)²)`) only to scale the pneumatic trail.
- Unmodified hit point is kept in `Tyre::unmodifiedContactPoint`, final one in
  `Tyre::contactPoint`, normal in `Tyre::contactNormal`.

---

## 7. Open questions / not resolved

1. **`ISuspension` vtable slot names.** Slots used by the tyre (+0x08 matrix, +0x10 point
   velocity, +0x18 force at point, +0x20 torque, +0x38 hub angular velocity, +0xc0 local
   force+torque) are named here from how they are used, not from the PDB. The suspension classes
   need their own map.
2. **`TyreModelData::Fz0`** is never written from tyres.ini in the code read (ini `FZ0` goes to
   `slipProvider.brushModel.data.Fz0` and `scTM.Fz0`); it appears to stay at the constructor
   default `2000.0`, and that value is what the relaxation-length and `USE_LOAD` wear formulas
   use. Confirmed in Task 17: nothing writes it from the ini, and `stepRelaxationLength` divides by
   2000.0 for every version. The port does the same and matches the game bit for bit.
3. **Old path calls `tyreModel->solve` too** (`addTyreForces`, after computing Fx/Fy from the
   brush provider). Resolved in Task 17: it is dead code. The `TyreModelInput` is only partly
   filled (`tyreIndex`, `cpLength`, `grain`, `blister`, `pressureRatio` and `useSimpleModel` are
   uninitialised stack), the returned struct is never read and `SCTM::solve` changes no state. The
   port leaves the call out.
4. **`BrushSlipProvider::getSlipForce` arguments**: resolved in Task 17 from the disassembly. The
   function reads only `slip` (+0) and `load` (+8) of `TyreSlipInput`. The caller also fills
   `friction`, `normalizedSlipX`, `normalizedSlipY` and `D`, and nothing reads them. The bool picks
   `asy` or 1.0 for `solveV5` and is ignored below `VERSION` 5 (section 5).
5. **`TyreThermalModel::buildTyre` / `getIMO` / `getPatchAt`** are saved but only skimmed: the
   patch neighbour wiring and the inner/middle/outer averaging were not worked through.
6. **Driven wheels:** `Tyre::step` skips `updateAngularSpeed` when `driven` is true; how
   `Drivetrain::step2WD/4WD` consumes `status.feedbackTorque` is outside this task.
7. **Who writes `Tyre::inputs`, `absOverride`, `aiMult`, `tyreBlanketsOn`, `status.inflation`**
   (brakes, ABS, AI, pit logic) was not traced.
8. **`SCTM::solve` constants** (`3 · 78.125`, `0.0064`, `0.12`, `0.8`, `1.1`) are transcribed as
   they appear; their physical meaning (patch half-length 0.08 m etc.) is my interpretation.
9. **`TyreDeflection`** has a PDB layout but no user was found among the tyre functions.
10. **Function count:** the MCP search caps output at 50 names; the remaining names were
    recovered with narrower searches and 65 of the 66 are identified.
11. **Tooling limits this run** (see the reply for details): the MCP's struct/script tools could
    not run, so struct layouts were read from `acs.pdb` directly with the DIA SDK, and callers
    in the file headers were computed from `acs.exe`'s call instructions and unwind table.
    Indirect (vtable) callers therefore show up as "pointer slots" rather than as named callers.

---

## 8. Updates from the car map (Task 05)

Later maps answered some of the questions above and corrected a few statements. Nothing above
was rewritten; this list says where the newer answer is.

- **Open question 1 (`ISuspension` slot names): resolved.** `docs/map/suspension.md` section 9:
  +0x08 `getHubWorldMatrix`, +0x10 `getPointVelocity`, +0x18 `addForceAtPos`, +0x20 `addTorque`,
  +0x38 `getHubAngularVelocity`, +0xc0 `addLocalForceAndTorque`. Static camber is not in the
  suspension geometry: `getHubWorldMatrix` rotates the hub matrix by the static camber angle
  before the tyre sees it.
- **Open question 6 (driven wheels): resolved.** `docs/map/drivetrain.md` 5.3: the drivetrain
  uses the sum of the driven wheels' `status.feedbackTorque` to accelerate the driven block and
  their difference for the differential, then writes `status.angularVelocity` back and calls
  `Tyre::stepRotationMatrix` itself.
- **Open question 7 (who writes the tyre's inputs): resolved** for `inputs`, `absOverride` and
  `aiMult`. `BrakeSystem::step` 0x14028e640, `EDL::step` 0x1402bb460 and `ERS::step` 0x1402930e0
  write `Tyre::inputs` (`docs/map/brakes.md` section 6); `ABS::step` 0x14028f610 and
  `AIDriver::stepSuperhuman` 0x1402a2a60 write `absOverride` (`docs/map/electronics.md`);
  `AIDriver::stepGasBrake` 0x1402a08e0 writes `aiMult` (`docs/atlas/ai_drivers.md`).
- **Section 4, step 8: one force does not go through the hub.** On a surface with
  `SurfaceDef::damping > 0`, `Tyre::step` applies `−damping · bodyVelocity · bodyMass` to
  `Car::body` at its origin (`IRigidBody` +0xf8), once per wheel on such a surface
  (`docs/map/track_surface.md` 5.5).
- **Sections 5 and 6: dead branches in this build.** `SurfaceDef::granularity` is set to 0 by the
  surface loader and never written again, so the three-sine bump pattern and the loose-surface
  special case of `addTyreForces` never run (`docs/map/track_surface.md`). `Car::torqueModeEx` is
  only ever `original`, so `Tyre::addTyreForceToHub` (+0xc0) and the hub reaction-torque branches
  never run either (`docs/map/steering.md`).
- **Section 6: ray details.** The per-wheel ray caster created in `Tyre::init` has length 3.0 (the
  provider fall-back uses 2.0); rays search only the static collision space, in first-contact
  mode with back-face culling. `SIN_LENGTH` is used as a spatial frequency (rad/m), not a length
  (`docs/map/track_surface.md`).
- **Section 4, step 1: the tyres.ini key table is now in the repository** as
  [`tyre_ini_keys.md`](tyre_ini_keys.md) (a copy of the Task 01 table that lived only in the
  git-ignored `re/tyre/`). Two things it does not cover: the setup items `PRESSURE_LF/RF/LR/RR`
  (psi) write `Tyre::status.pressureStatic` (`SetupManager::initItems` 0x140289570), and
  `[COMPOUND_DEFAULT] INDEX` is not read by the tyre loader at all (only by `AIDriver::AIDriver`
  0x140298c60 and the setup UI).
