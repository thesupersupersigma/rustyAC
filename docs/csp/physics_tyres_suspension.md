# Physics extensions: tyres, suspension, chassis springs and dampers (hooked code)

CSP 0.3.0-preview649 (build 4184) does not sit beside acs.exe's tyre and suspension code; for the tyres it **runs its own copy of `Tyre::step` for every car that has CSP's per-car object** (the object exists for non-extended cars too: proven by the gas-law pressure of the stock F2004 recording, see section 1) and only falls back to the original body when the object is missing. The suspensions are different: the four classic types are wrapped, a fifth type (COSMIC, a body-and-joint model) is built by CSP in place of a `Suspension` object, and a "DWB2" layer can take over the spring / damper / bump-stop maths of double-wishbone cars. This file maps the 60 table rows of group `physics_tyre_susp` (43 hooks + 17 lookups; 4 of the "lookups" are really hooks, see 2.0) plus the CSP-own functions they call.

All dwrite.dll addresses are RVAs (image base `0x180000000`); acs.exe addresses are the `0x14...` VAs of `docs/map`. Evidence markers: `[dll]` seen in the decompile / bytes / tables, `[log-csp]`, `[ini]`, `[sdk]`, `[lua]`, `[prior]` earlier task's notes, `[interp]` my inference. Classes: **A** pass-through that only records / forwards, **B** wraps the original and changes inputs or outputs, **C** replaces the original (original never, or only for objects without the CSP object), **D** lookup (CSP calls the acs function through its slot).

## 1. What it does

What changes, in order of how sure I am. "Stock car" = a Kunos car with `VERSION=1/2`, no `[_EXTENSION]`, no `extended-N`.

1. **A stock car already runs CSP's tyre step.** The hooks look the per-car object up by car index (`_DAT_1826ae290[car+0x3e9c]`, or `*(info+0xe0)` while the car loads); if non-null, `Tyre::step`, `getCorrectedD`, `reset`, `updateAngularSpeed`, `getDY/getDX`, `addTyreForcesV10`, `stepTyreBlankets` ... run CSP code. Proof from behaviour, not from the table: the owner's recording of the stock F2004 under CSP has a **gas-law tyre pressure** (`10.97 + 0.084 x (core - 26)`, [prior] `docs/port/grip_investigation.md` suspect 3) while plain acs.exe gives `10 + 0.16 x (core - 26)`; that formula is computed at the end of CSP's `Tyre::step` (2.2, phase 19). The CSP log of the owner's last session (2026-10-12; a plain third-party DWB car, `rss_formula_hybrid_2020`, no `[_EXTENSION]`, no Lua, no `extended-N`) also prints "Cphys Drivetrain Initialized.", the Embree build, the per-compound tyre classification and "Loading tire compound #0..#5" for that non-extended car [log-csp]. [dll][prior][log-csp]
2. **Tyre pressure law (proven).** `P_dyn = (T_core + 273.15) / (T_ref + 273.15) * (P_static + P_atm) * g1 * g2 - P_atm`; `P_atm` = ambient pressure at the car's altitude in psi (kPa * 0.145038, CSP function `0xaf75f0`, same barometric family as the air-density law `0xaf7760`), `T_ref` = a float at `*(car+0x138)+0x100` (14 C in the owner's session, i.e. the ambient / setup temperature [interp]), `g1` from a per-compound curve (1.0 when absent), `g2` a tyre-growth term (1.0 when the gain is 0). Check with the numbers of that session: P_static 10 psi, altitude 418 m (P_atm about 13.98 psi), T_ref 14 C gives slope `(10 + 13.98) / 287.15 = 0.0835` psi/C and `11.00` psi at 26 C: the recorded fit is 0.084 and 10.97 [dll][prior][calc].
3. **Brake torque limit (BRAKES_LIMIT-like).** When the byte at `0x2695086` is set (behaviour matches hidden `[PHYSICS_EXPERIMENTS_DONE] BRAKES_LIMIT=1`, on by default) the brake / handbrake torque that opposes wheel spin is clamped so that one step cannot push the wheel through zero speed; stock AC subtracts the full brake torque. Stock cars. [dll][interp]
4. **Contact search is CSP's.** The single downward ray of a tyre goes through CSP's batched ray executor (`0x6b1950`; Embree by default per `general.ini` `ODE_BATCH_RAYCAST`; profiler scope "Car #1/Tyre/Raycasts" in `Tyre::step`; length 3.0 m, as AC's caster) instead of AC's `IRayCaster` unless the caster/provider pair is missing; for **car index 0 only** an optional multi-ray grid replaces it: `(2*LATERAL+1) * (2*LONGITUDINAL+1)` rays (63 with the shipped defaults 3 / 4, 119 when the low-speed "ray doubling" applies), each weighted by depth and optionally by the local grip (`depth * getDY * getDX`), giving an averaged contact point / normal and a per-tread-slice load profile for the thermal model. Surface noise (sine / Perlin) can shift the ray origin instead of the hit (`TYRE_SHIFT_SURFACE_PROPERLY`, byte `0x2695094`). [dll][ini]
5. **The V10 tyre force and `SCTM::solve` are CSP implementations** (10,040 B and about 4.2 KB, not decompiled). Whether they equal AC's for stock data is **not proven** (2.7, open question 1). The old brush path (`VERSION < 10`) stays AC's own `addTyreForces`, with modified inputs. [dll]
6. **Tyre grip corrections that are new for every car:** `Tyre::getCorrectedD` (CSP's version has no pressure penalty term, see open question 2), `stepDirtyLevel` post-processing (per-wheel force multipliers, "forceless tyres" 0.0001, per-tyre grip loss byte `Tyre+0x41a` = `1 - b/255`, and a wet-track / temperature grip law when the per-car "rain physics" flag `ext+0xb4` is set). [dll][interp]
7. **Thermal model**: version 0 = AC's 12 x 3 patch model (original step + a per-wheel multiplier and CSP's own practical-temperature / grip-curve code); versions >= 2 = CSP computes the thermal input itself and drives a layered carcass model (Thermal Model V4, `PERFORMANCE_MAP` 2D lut, bulk / sidewall layers, rim, brake). Blankets, reset temperature and the per-axle `OVERRIDE_BLANKETS_TEMP` follow the layered model. [dll][prior]
8. **Compounds are added by CSP:** "virtual" rain tyres created from the first slick (`Creating rain tyres from: ...`), `ICE-R` / `ICE-S` ice compounds, type classification (`TYPE_HINT`: SLICK, SEMISLICK, TREADED, RACING_TREADED, VINTAGE_PERFORMANCE; name heuristics `street`, `intermediate`, `wdt enjoy`, `cup 2`, `pilot sport`, `super`), generated sections carry `_VIRTUAL_ORIGIN` in the in-memory tyres.ini. The F2004 under CSP shows 4 compounds ([prior] P18). [dll][log-csp][lua][prior]
9. **Suspension classes:** tag byte at `ISuspension+0x36` (1 DWB, 2 STRUT, 3 AXLE, 4 ML, 5 COSMIC); `Suspension::Suspension` hands construction to CSP when the car has a registered COSMIC wheel; CSP adds `_EXTRA_LINK` rigid links and `_AC_LINK_ALTERATION` joint edits to **any** suspension type (built when the 4th wheel attaches); damage amount can only grow (stock lets a weaker hit lower it); negative anti-roll rates act; STRUT `getSteerBasis` can use another strut point (`FIX_STRUT_FFB`); DWB steer torque is scaled by `|cos|` between the force and the steering rod; DWB hub matrix gets a per-wheel extra camber; strut damper speed sign is flipped by a thread-local marker. [dll][interp]
10. **Lua**: the physics-thread script sees 36 per-wheel tyre fields, 6 damper fields, 13 surface fields and has 21 `ac.*` plus 2 `physics.*` tyre / suspension / joint functions (3.3). [sdk]

## 2. Game parts it replaces or extends

### 2.0 Reading guide and counts

* 60 rows in `hook_groups.tsv` for this group: Tyre 17 hooks / 7 lookups, TyreThermalModel 8 / 2, Suspension 5 / 1, SuspensionStrut 4 / 1, SuspensionAxle 3 / 0, SuspensionML 3 / 0, AntirollBar 1 / 0, Damper 1 / 0, HeaveSpring 1 / 0, BrushSlipProvider 0 / 1, SCTM 0 / 1, SuspensionAnimator 0 / 3, SuspensionAvatar 0 / 1 (= 43 hooks, 17 lookups).
* **Correction to the table (finding for the lead).** 76 of the 507 table rows marked "lookup" register a replacement function in a second stub form: the stub starts with `lea rdi,[replacement]` before the usual record write, and the extractor missed it. Four are in this group and are **hooks**: `SCTM::solve` (replacement `0x69d700`, 196 B, slot `0x268b510`), `Suspension::step` (`0x11232b0`, 4,808 B, slot `0x268c410`), `SuspensionAnimator::addModel` (`0x6a3320`, 97 B, slot `0x268b610`), `SuspensionStrut::getSteerBasis` (`0x6a3900`, 321 B, slot `0x268b620`). Each replacement calls the original through the row's slot (checked by scanning the bytes). The 76 are (the four of this group plus): AeroMap::step / getCurrentLiftKG / getCurrentDragKG, ERS::step, Kers::step, ACChat x2, ACSuspension::render, DIControlButton x3, ESCMenu x2, FormChat::onChatMessage, JoypadCarControl x2, KeyboardCarControl x2, ksgui::Taskbar x4, ksgui::TaskBarIcon::render, NetCarStateProvider x3, Node x3, PauseMenu x2, ReplayInterface::render, SetupGearManager::addSetupItem, SetupInformation::getTOE, AIDriver x5, CarPedalsForm::render, DICarControl x2, CubeMap x3, KN5IO x2, ksgui::Control::render, ksgui::Spinner x2, ParticleSystem::step, SetupScreen x4, SetupTab::addItem, StaticParticleSystem::finalize, ACTimeFlow::render, Track::initAISpline, MaterialFilter(SM)::apply, NodeBoundingSphere::render, ReplayRecorder x3, Sim x5, SkinnedMesh::render. [dll]
* Corrected group totals: **47 hooks + 13 lookups**.
* Per-car object ("ext") offsets used below (all [interp] names, offsets [dll]): `+0xb0` car index (0 = the car with ext index 0), `+0xb4` byte "rain physics / wet-grip law on", `+0xb7` byte `FIX_STRUT_FFB` tri-state (0xFF = use global byte `0x26a2c04`), `+0x1b8` DWB2 object (null unless the car uses CSP's custom DWB), `+0x210` **tyre system "TM"** (always non-null when ext is: Tyre::step dereferences it unchecked), `+0x248` extra-links / link-alteration object, `+0x250/+0x258` script holders, `+0x308` override object (byte `+8` = active; wheel-speed, pressure overrides), `+0x318` `Car*`, `+0x320` car record. TM fields seen: `+0x8..+0x10` vector of per-compound records (0x5f8 bytes each) and `+0x20` the current compound's record (`+0xe0` per-axle blanket override, FLT_MAX = none; `+0xe8 + tyreIdx*8` the layered model of that tyre; `+0x188/+0x190` per-axle numbers), `+0x238` int thermal-model version (0 legacy, 1, 2, 3, >=4), `+0x240` int number of compound records, `+0x74c` int lateral rays, `+0x750` int longitudinal rays, `+0x754` float max ray angle (deg), `+0x7a8` byte "no ray doubling", `+0x7a9` byte "weight rays by grip", `+0x7aa` byte (normal blend, unidentified), `+0x2210[w]` thermal-input multiplier, `+0x2280[w]` byte per-wheel "dynamic-grip undo" flag, `+0x21c0/+0x21d0` per-wheel force multipliers.
* Tyre fields used (offsets from the hooks; names follow `docs/map/tyre.md`, status block at `Tyre+0x2e8`): `+0x1c` angular inertia, `+0x2f0` camberRAD, `+0x2f8` slipRatio, `+0x2fc` angularVelocity, `+0x300` Fy, `+0x304` Fx, `+0x308` Mz, `+0x30c` isLocked, `+0x32c` rollingResistence, `+0x334` feedbackTorque, `+0x338/+0x33c/+0x340` loaded / effective / live radius, `+0x344/+0x348` pressure static / dynamic, `+0x350` virtualKM (double), `+0x358..+0x360` lastTempIMO, `+0x368/+0x370` grain / blister (double), `+0x378` inflation, `+0x380` flatSpot, `+0x40c` default (compound) pressure, `+0x41a` grip-loss byte, `+0x41c` absOverride, `+0x420` TyreThermalModel (0xe0), `+0x58` modelData.version, `+0x3a0` hub (`ISuspension*`), `+0x4e1` the thermal model's tyre index byte, `+0x5a1` driven, `+0x5a4` flag word (bit 1 = "forceless", bit 2 = hub proxy), `+0x5a8` rayCaster, `+0x620` ray provider, `+0x628` wheel index, `+0x640` `Car*`, `+0x654` car index (CSP), `+0x829` "smooth load sensitivity" byte (CSP).

### 2.1 `Tyre` (acs 0x14026dbd0..0x140285070; 17 hooks, 7 lookups; 152,494 B of pseudo-C, `Tyre::step` alone 111,669 B / 2,544 lines, all read)

| Symbol | acs VA | CSP RVA (slot) | Cat | What it changes, and when |
|---|---|---|---|---|
| `Tyre::Tyre` | 0x14026dbd0 | 0x6a9b30 -> body 0x6a9b60 (0x268b778) | B | 1,115-byte body calls the original constructor (call at 0x6a9bbc) then sets CSP state: the load-sensitivity interpolation byte `+0x829` (log "Using linear interpolation on tire load sensitivity curves" when it is off; the next string in the pool is the key name `SMOOTH_LOAD_SENS`), then `0x6b6b80` (post-construct registration). Every car. The F2004 log has no "linear interpolation" line, so the default is the smooth (cubic) mode [interp]. |
| `Tyre::~Tyre` | 0x14027d320 | 0x2db1f0 (0x26a7558) | C | empty `ret` stub (shared with 38 other symbols); tyres are never destroyed [interp]. |
| `Tyre::initCompounds` | 0x140280800 | 0x6a9fc0 (0x26a7560) | B | With ext: wraps the original with a scope closure (`0x6b7f10`, runs after it; not decompiled, probably CSP's own re-read of the compounds, see 2.6). Afterwards for **all** cars: `Tyre+0x40c` (cold / default pressure) := `compoundDefs[currentCompound].+0x358` (20.0 if there are none, compound 0 if the index is out of range); and with byte `0x2695091` (behaviour = `RANDOMIZE_WHEEL_ANGLES`) the wheel's `angularVelocity` is temporarily set to `rand*100+1`, the original `stepRotationMatrix` is called with the car's sleep counter forced to 0, then both are restored: each wheel starts with a random rotation of its mesh. |
| `Tyre::updateAngularSpeed` | 0x140284fa0 | 0x6aa310 (0x268b748, never called) | C | Two paths. **COSMIC wheel** (suspension tag 5 with a wheel body): angular speed is read from the wheel rigid body in the ODE world (scalar projection of the body's relative angular velocity on its spin axis, `isLocked = abs(w) < 0.1`) and the rolling-resistance torque is applied to the body with `addTorque`. **Every other wheel**: AC's own integration, line for line (`w += feedbackTorque / inertia * dt`, sign change locks the wheel, `abs(w) < 1 -> *0.9`), after calling CSP's `updateLockedState`; so stock-equivalent [dll, compared with `vanilla_tyre.rs`]. |
| `Tyre::reset` | 0x140283380 | 0x6aa5a0 (0x268b750) | C with ext, else B | With ext: own reset (pressures := `+0x40c`, locked, inflation 1, grain / blister kept as `lastGrain/lastBlister`, hub matrix re-read, `lastTempIMO` from the thermal patches (legacy) or from the layered model, `virtualKM` 0, layered model reset via `0x6b9fa0`, clears four per-wheel float vectors of the TM). Without ext: original. In both cases fires CSP event list `0x26910f0` with `{car index, wheel}`. |
| `Tyre::getCorrectedD` | 0x140280190 | 0x6aab80 (0x268b730) | C with ext | `TyreThermalModel::getCorrectedD * wearCurve(virtualKM)` and writes the wear multiplier through the optional pointer. **The pressure-penalty divisor of the original (`1 / (abs(P_dyn - P_ideal) * PRESSURE_D_GAIN + 1)`) is not in this function** [dll, vs `vanilla_tyre.rs peek_corrected_d`]; whether the same term lives elsewhere in CSP's force code is unknown (open question 2). |
| `Tyre::updateLockedState` | 0x140285070 | 0x6abfc0 (0x268b740, never called) | C with ext | Same logic as stock (locked while `max(abs*brake, handbrake) >= abs(loadedRadius*Fx)`, `abs(w) < 1`, not driven) [dll]. |
| **`Tyre::step`** | 0x140283800 | 0x6ac2a0 (0x268b728) | **C with ext** (original only without ext) | Full CSP step, phases in 2.2. Calls original **through slots**: `addTyreForces` (0x268b738, `VERSION < 10`), `stepRotationMatrix` (0x268b790), `stepGrainBlister` (0x268b760), `stepFlatSpot` (0x268b768). Calls CSP hooks directly: `addGroundContact` 0x6b2dc0, `addTyreForcesV10` 0x6b40e0, `updateAngularSpeed` 0x6aa310, `updateLockedState` 0x6abfc0, `stepThermalModel` 0x6b2640, `getDY/getDX` 0x6b68c0 / 0x6b6a00. Profiler scopes (car with `physicsGUID == 0`): "Car #1/Tyre/Raycasts", "/Post RC", "/Ray processing", "/Precise sine noise", "/Thermal model". Guard: "Should Not Reach: Wheel Angular Velocity Infinite" (angular velocity NaN/Inf -> 0). |
| `Tyre::stepDirtyLevel` | 0x1402843d0 | 0x6b22e0 (0x268b720) | B | Original first, then **post-processes Fx, Fy, Mz, Dy, Dx** (still before the force reaches the hub; the V10 path calls this hook once): (a) `VERSION < 10` and TM per-wheel flag: Fx, Fy divided by the track's `dynamicGripLevel` (`PhysicsEngine+0x198 -> +0x128`), undoing AC's own factor; (b) TM post hook `0x8d76d0`; (c) with ext `+0xb4`: factor `1 + [(0.025 - T^2 * 2.44e-5) - 0.5*(1-g) + T^2*(1-g)*4.88e-4] * (1 - s^2)`, `g` = dynamic grip, `T` = `PhysicsEngine+0x104`, `s` = logistic function of `(Tyre+0x62c - 1.7) * -3.3`; stored in the value block (`+0x264+wheel*4`) and applied to Fx, Dy, Dx, Fy [interp: wet / cold grip law of RainFX]; (d) per-wheel multipliers from TM (`+0x21c0`, `+0x21d0`); (e) `flags & 2` (forceless) -> x 0.0001; else byte `+0x41a` != 0 -> x `(1 - b/255)` ("awake in pits" grip loss [interp]). |
| `Tyre::stepThermalModel` | 0x140284ca0 | 0x6b2640 (0x268b710) | B (TM v0/1) / C (TM v>=2) | See 2.3. |
| `Tyre::stepTyreBlankets` | 0x140284f10 | 0x6b2ce0 (0x268b718) | C with ext | Blanket temperature := per-axle override (`TM->+0x20->+0xe0`, `OVERRIDE_BLANKETS_TEMP`; FLT_MAX = none) else `min(Tyre+0x84c, Tyre+0x48)`; written into all legacy patches (`+0x18` of each 0x28-byte patch) **and** every layered-model temperature (surface, three zones, core; `0x8fc780` re-seeds) of this tyre (index `Tyre+0x4e1`) in every compound record. Runs only while `Tyre+0x64c` (blankets on) and clears it above 2.78 m/s (10 km/h). |
| `Tyre::addGroundContact` | 0x14027d980 | 0x6b2dc0 (0x268b700) | B | With ext -> CSP body `0x6b2e90` (4,532 B, not decompiled): calls the original (call at 0x6b2f42) plus extras; it also uses the hub proxy builder `0x6ac1c0`, `getCorrectedD`, the value block `0x7381c0` and a TM function (`0x8d8d70`). Without ext: original. |
| `Tyre::addTyreForcesV10` | 0x14027ed60 | 0x6b40e0 (0x268b708) | **C with ext** | -> CSP body `0x6b4170` (10,040 B, not decompiled): never calls the original (slot unused there); calls CSP `getCorrectedD`, `stepDirtyLevel`, original `stepPuncture` (slot 0x268b770), original `addTyreForceToHub` (slot 0x268b6e0, used for `TORQUE_MODE_EX`), CSP's SCTM-like solver `0x69c4f0` (+ `0x69c6fe`), powf / expf. Without ext: original. |
| `Tyre::addTyreForceToHub` | 0x14027dc00 | 0x6b68b0 (0x268b6e0) | A | 7-byte jump to the original; exists so CSP code can call it by name (`jmp [slot]`). |
| `Tyre::getDY` | 0x1402803d0 | 0x6b68c0 (0x268b6e8) | C | Same formulas as stock (linear `Dy0 + Dy1*load*0.0005`, exponent law, or the load-sensitivity curve) divided by `1 + 0.2*blisterShare`. Differences: no `load > 0` early-out; the curve is evaluated with cubic spline **after clamping the load to the curve's range** when `+0x829` is set (stock cubic extrapolates), or with linear interpolation (`Curve::getValue`, itself hooked) when it is clear. Unconditional (no ext test). |
| `Tyre::getDX` | 0x140280240 | 0x6b6a00 (0x268b6f0) | C | Same as `getDY` for the longitudinal curve (`+0x1b8` curve, `+0x244` exponent, `+0x240` multiplier, `+0x64/+0x68` linear terms; `getDY` uses `+0x138`, `+0x23c`, `+0x238`, `+0x5c/+0x60`). Unconditional. |
| `Tyre::setCompound` | 0x1402834e0 | 0x6b6b40 (0x268b6f8) | B | Original, then if the thermal model's performance curve (`Tyre+0x480/+0x488` vector) is empty calls the Curve helper `0x5bb330` (118 B; with the removed `PERFORMANCE_CURVE` key of CSP cars this builds a default curve [interp]); returns the original's bool. |
| lookups (D) | | | D | `getDynamicK` 0x140280560 (0x268b788; called by the RaceEngineer ride-height hooks 0x10ebbd7 / 0x10ebd71 and 0x14d0dbf), `stepRotationMatrix` 0x140284b80 (0x268b790; Tyre::step, initCompounds, Drivetrain 4WD hooks), `stepRelaxationLength` 0x140284a40 (0x268b758; no CSP caller found), `stepGrainBlister` 0x140284600 (0x268b760), `stepFlatSpot` 0x140284500 (0x268b768), `stepPuncture` 0x1402849b0 (0x268b770), `addTyreForces` 0x14027e1a0 (0x268b738). |

### 2.2 Inside `Tyre::step`: CSP order against AC's order (ext path)

AC order (`docs/map/tyre.md` 3): hub matrix -> one ray -> `addGroundContact` -> `addTyreForces` / `V10` -> feedback torque -> `updateAngularSpeed` + `stepRotationMatrix` -> `stepThermalModel` (+ `addThermalInput`, `TyreThermalModel::step`, blankets) -> `stepGrainBlister` -> `stepFlatSpot`. CSP's, numbered by the code order; "all" = stock cars too, "ext+" = needs a key / version, "car 0" = only the car with ext index 0.

| # | Phase | Scope |
|---|---|---|
| 0 | Profiler scope; ext lookup by car index. No ext: original `Tyre::step` + debug draw, end. | all |
| 1 | TLS slot `+0x4e0` := this tyre (read by the `SCTM::solve` hook and CSP helpers); restored at exit. | all |
| 2 | Wheel 0 only: `0x858a30` on both script holders adds the script's queued steer-torque accumulators (two floats at holder state `+0x1188/+0x118c`) to the front hubs' `steerTorque` (the script API `ac.addHubForce2/addTorque(.., addSteerTorque)`) [interp]. | all |
| 3 | `flags & 4`: `Tyre+0x3a0` (hub pointer) := CSP proxy from `0x6ac1c0` (TLS-cached 0x40-byte object forwarding to the real hub; vtable `0x2221f98`); restored at the end. Same proxy is used by `addGroundContact`. [interp: "forceless / virtual" hub] | ext+ |
| 4 | Per-wheel rates into ext arrays: `(w - w_old) * r_eff / dt` (`ext+0x458[w]`), slip-ratio rate (`ext+0x468[w]`, low-passed with `ext+0x478[w]`, time constant from `ext+0x1f8->+0x2c`). Used by script / controller inputs ("SLIP_ACCEL" style). | all |
| 5 | Vertical stiffness conditioning (TM present): `k_total = (data.k + (P_dyn - P_static) * gain) * camberLUT * mult`, series with the sidewall rate `TM+0x7ac[axle]`, low-passed with a speed-dependent relaxation (`relax / max(abs(v), 5)`), result written back into `data.k` and into the value block `+0x225c`. All factors are 1 or infinite for stock data [interp], so this is an identity for a stock car. | all (identity), ext+ |
| 6 | Blow-out check: when the timer `ext+0xc4` has run out and any of three thresholds is reached (`ext+0xbc` against a per-wheel value of the car record `+0xd94`, `ext+0xc0` against `virtualKM`, `ext+200` against a car-record value) `inflation := 0`. Thresholds' source unknown. | ext+ [interp] |
| 7 | Eight model fields saved (`Tyre+0x6c, 0x7c, 0x564, 0x67c, 0x680, 0x684, 0x20 (only TM v<1), 0x448`); TM applies its per-step overrides (`0x8d81a0`: falloff, SAT, camber and sidewall terms) into the tyre; they are restored in phase 20. Stock data -> TM leaves them as they are [interp]. | all |
| 8 | Zero per-step outputs; hub `getHubWorldMatrix` (+8) -> `worldRotation`, position, axes; NaN guard on angular velocity. | all |
| 9 | Ray counts: `iLat = TM+0x74c`, `iLon = TM+0x750`, only when `ext+0xb0 == 0`, `DAT_1826b8a18 == 0`, and `ext+0x308` object not disabling; below 10 (units of `Car` speed getter) and `TM+0x754/(iLon+1) > 2` degrees and not `+0x7a8` -> `iLon *= 2`. Total rays `(2*iLat+1)*(2*iLon+1)`. Other cars: single ray. | car 0, ext+ |
| 10 | Surface-noise pre-shift (`TYRE_SHIFT_SURFACE_PROPERLY`, byte `0x2695094`) via `0x6aadc0`; script offsets from the `ext+0x310` bridge. | all |
| 11 | **Ray cast**: no ray provider (`Tyre+0x620`) -> AC's `rayCaster->cast` (vtable +8, length argument 2.0); else batched executor `0x6b1950` with 1 ray (length 3.0, origin = wheel centre, direction -Y) or the multi-ray grid. Grid: per-ray tilt in two axes from the lateral / longitudinal index, hit depth `r - dist`, weight `depth` or `depth*getDY*getDX` (`+0x7a9`), accumulation of weighted point and normal, per-tread-slice load vector (`ext+0x3d8+w*0x10`, `1/N` normalised, passed to the layered thermal model when TM version > 3), hit count; optional "extra contact points" (per-car flag table `0x1826a9460[physicsGUID]`, byte `0x26950a6`) and rim contact (`0x8d6020`, `0x8d6090`). Ray-mode bytes `0x269507c` (custom raycasting on) and `0x269507f` (value 2 = Embree). | all / car 0 |
| 12 | Hit post-processing: if the squared distance origin-to-hit is at least `(2 * radius)^2`, or the car body query (vtable +0x68 of `*(Car+0x118)`) returns a y component below 0.2 [interp: car on its side], a second straight-down ray of length 3.0 from the same origin is cast and its hit becomes `unmodifiedContactPoint` (`Tyre+0x3e8`); normal-vs-wheel-up dot test 0.96 as AC's; surface noise **after** the hit (the stock place, when phase 10 did not shift the origin); byte `Tyre+0x5a2` := `(abs(dot) - effectiveRadius) * 100` clamped 0..254, 0xFF when airborne; debug lines "Tyres" (`ac.PhysicsDebugLines.Tyres`). | all |
| 13 | `Tyre::addGroundContact` (CSP wrapper -> original + extras). | all |
| 14 | Radius rate bookkeeping (`ext+0x460/+0x470/+0x480`), brake torque magnitude cache (`ext+0x2120`). | all |
| 15 | Force: `modelData.version < 10` -> original `addTyreForces` through the slot; else CSP `addTyreForcesV10` (-> `0x6b4170`). | all |
| 16 | Wheel torque sum into `feedbackTorque` (`+0x334`): rolling resistance (`+0x32c`) + electric torque (`inputs +8`) + script-added torque (`ext+0x2270[w]`) + tyre reaction (`VERSION < 10`: `loadedRadius * Fx`; else `- Tyre+0x854` = `localMX`) - brake term; the brake term is the **limited** one (clamped so the speed cannot cross zero within `dt`) when byte `0x2695086` is set and the wheel is not COSMIC; stock-style full brake otherwise. NaN/Inf -> 0. Torque added by `ac.addWheelsFeedbackTorque` (`Tyre+0x53c`) is consumed in the driven branch (phase 17). | all |
| 17 | Non-driven wheel: CSP `updateAngularSpeed` (COSMIC: wheel body) or an override from `ext+0x308` (wheel speed from ground speed / script; modes at `+0x398/+0x39a/+0x39c`); driven wheel: `updateLockedState` (non-COSMIC), `oldAngularVelocity := w`, script torque added. Then original `stepRotationMatrix` (non-driven). | all |
| 18 | Below 10 (speed value at `Tyre+0x614`) the slip factor `Tyre+0x310` is multiplied by `abs(0.1*v)`; TM end-of-step `0x8d4970`; **CSP `stepThermalModel`** (2.3) called directly. | all |
| 19 | **Gas-law pressure** (formula in section 1.2); optional override from the `ext+0x308` object (`+0x400`). Then original `stepGrainBlister`, `stepFlatSpot`. | all |
| 20 | Restore the eight model fields; export block `0x7381c0` (car 0 only): `+0x174/+0x17c` per-axle effective radius and deflection in mm, `+0x194+w*4` slip angle in degrees, `+0x2460+w*4` dynamic pressure, `+0x2400..+0x242c` four contact points in car space, `+0x22b0` contact-length figure. | all / car 0 |
| 21 | For tags 2, 4, 5 (STRUT, ML, COSMIC; not DWB or AXLE): get the steering axis (`+0xa8`), hub matrix (+8) and hub angular velocity (+0x38), form a vector from the spin axis and the hub's angular velocity (cross product, scaled) and apply it with `addTorque` (+0x20) to the suspension; stored in `ext+0x498[w]` [interp: gyroscopic precession torque of the spinning wheel]. COSMIC with wheel body: also exports steering-related values to the car record (`+0xe48`, `+0xf44`, `+0xf48`). Restore hub pointer and TLS. | ext+ |

### 2.3 `TyreThermalModel` (acs 0x14026df80..0x1402ae340; 8 hooks, 2 lookups; 16,190 B)

The model object (0xe0 bytes in acs) carries CSP data in its padding: `+0xc0` byte "thermal multiplier valid", `+0xc1` byte tyre index 0..3 (a static counter `& 3` at construction), `+0xc4` float multiplier, `+0xc8` float practical temperature, `+0xcd` byte, `+0xdc` ushort car index.

| Symbol | acs VA | CSP RVA (slot) | Cat | What it changes |
|---|---|---|---|---|
| `TyreThermalModel::TyreThermalModel` | 0x14026df80 | 0x6b93a0 (0x268b7f0) | B | Original, then `+0xdc` := index of the car being loaded (`*_DAT_1826a9d50`), `+0xc1` := counter & 3, `0x6ba300` init. |
| `getCorrectedD` | 0x1402adbc0 | 0x6b8fc0 (0x268b7e0) | C with ext | Equal to stock (`d * thermalMultD` when active) **except** that with the layered model (TM version > 0) and a compound that has a performance map / curve it returns 1.0 (the map applies the grip elsewhere). |
| `getCurrentCPTemp` | 0x1402adbe0 | 0x6b9070 (0x268b7e8) | C if TM version > 3, else original | Contact-patch temperature as the weighted mean of the layered model's surface temperatures with the per-slice load weights. |
| `getPracticalTemp` | 0x1402ae140 | 0x6b94a0 (0x268b7a8) | B/C by version | v1 and no layered data -> contact-patch temperature; v2-3 -> ring-sampled by wheel phase; v>=4 -> layered model's own (Ghidra's return type is unreliable here; the float result is lost). |
| `step` | 0x1402ae340 | 0x6b98d0 (0x268b7b0) | B | TM v0: original step, then practical temperature `+0xc8` and `+0xc4 := performanceCurve(T - offset)`; v>=1: no original step, `+0xc4` from the layered model's offset `+0x358`. |
| `init` | 0x1402ae170 | 0x6b9d00 (0x268b7b8) | C with ext | -> `0x6b9d80` (82 B). |
| `reset` | 0x1402ae1c0 | 0x6b9fa0 (0x268b7c0) | B | Original, then (TM v > 0) all legacy patches and every layered temperature := `PhysicsEngine+0x104` (road/ambient temperature field); `ext+0xc4 := 1.0`; per-wheel previous-radius cells reseeded. |
| `addThermalInput` | 0x1402ad090 | 0x6ba0f0 (0x268b7a0) | C with ext | -> `0x6ba160` (404 B). |
| lookups | | | D | `addThermalCoreInput` 0x1402ad070 (0x268b7d0; called by CSP `stepThermalModel`), `getAvgSurfaceTemp` 0x1402adb90 (0x268b7d8; layered-model code). |

`Tyre::stepThermalModel` (2.1 row, 0x6b2640, 8,591 B) in detail, per TM version `+0x238` [dll]:

* **no ext / v0 / v1:** the thermal friction constant `Tyre+0x20` is multiplied by the wheel's `TM+0x2210` (clamped >= 0.001) around the **original** `stepThermalModel`; afterwards `0x8d5160` (a TM step).
* **v >= 2:** the original is not called. `thermalInput (+0x330) = sqrt(slideX^2 + slideY^2) * TM+0x2d0 * TM+0x2210 * Tyre+0x20` (NaN guard "INFINITE TIRE THERMAL INPUT"); for `data.VERSION < 5` rolling-heat term `Tyre+0x24 * w * load * 0.001` is added, for `VERSION > 5` a slip-power term; the layered carcass model gets `(load / ..., camber in degrees, patch-contact fraction)`; with `lateral rays < 1` the legacy `addThermalInput` is used instead; `VERSION >= 5` also feeds `addThermalCoreInput`; then CSP's `TyreThermalModel::step`; blankets if the car's blanket flag (`car+0x138 -> +0xb8`) is set; layered model step `0x8fa810`; `Tyre+0x4e4 := layered +0xfc` (carcass temperature exported to Lua as `tyreCarcassTemperature`).

### 2.4 Suspension classes (acs 0x1402c0720..0x1402cab00)

Common: every constructor hook sets the **tag byte** `ISuspension+0x36` (padding of the 0x38-byte base) and a ushort `+0x34 := 0`: DWB 1, STRUT 2, AXLE 3, ML constructor 2 then `attach` sets 4 (copy of the STRUT value, harmless because `attach` follows [interp]), COSMIC 5 (set by CSP's factory). The `attach` hooks of DWB, STRUT, AXLE and ML are one piece of code: run the original, set the tag, and when the **last wheel** of the car attaches (DWB `+0x1c8 == 3`, STRUT `+0x188 == 3`, ML `+0x68 == 3`, AXLE `+0x38 == 1` = right instance) and the extra-links object `ext+0x248` has no joints yet: (1) parse its config lazily (`0x6966a0`), (2) if "print out" is set and not yet printed, log "Printing out original AC links" and every joint (`0x6831b0`), (3) create every `_EXTRA_LINK` record (52-byte records, `0x84af80`, 87 B) and keep the joint pointers in a vector, (4) apply every `_AC_LINK_ALTERATION` record (48-byte records, wheel bitmask in the first dword) to the joint lists of the chosen wheels (`0x84b9a0`, 178 B): DWB joints at `+0x68..+0x90` (5), STRUT `+0x58..+0x80` (5: three distance, slider, ball), AXLE vector `+0x88/+0x90` stride 0x40 (two joints per link), ML vector `+0x70/+0x78` stride 0x38. Works for every suspension type, stock cars included, when the config has such records.

| Symbol | acs VA | CSP RVA (slot) | Cat | What it changes |
|---|---|---|---|---|
| `Suspension::Suspension` (DWB) | 0x1402c0720 | 0x1122cc0 (0x268c400) | B / C | With ext and a **registered factory for this wheel** (table at `0x1826974d0`, key = wheel index; filled by the COSMIC loader 0x87e090 "Creating cosmic suspension for wheel N"): tag := 0 and CSP's constructor runs **instead of the original**; otherwise original + tag 1. Then `0x124580(this+8)`. This is how `TYPE=COSMIC` cars get CSP objects while acs.exe's `Car::Car` still allocates `Suspension` objects (the car-core hook `0x595b70` maps the type string, see `physics_car_hooks.md` 2.1) [interp]. |
| `Suspension::getHubWorldMatrix` | 0x1402c4c50 | 0x1122db0 (0x26ad0f8) | B | Tag 1 with a DWB2 object (`ext+0x1b8`): `staticCamber += ext->extraCamberDeg[wheel] * pi/180` for the duration of the original, then restored: the tyre and the graphics see the extra camber. |
| `Suspension::attach` | 0x1402c0fb0 | 0x1122ea0 (0x26ad100) | B | Extra-links builder, see above. |
| `Suspension::setDamage` | 0x1402c31e0 | 0x1122f40 (0x26ad108) | C with ext | `damageAmount = min(max(damageAmount, (impact - minVelocity) * gain), maxDamage)` (stock: the new value **replaces** the stored one, so a weak hit after a hard one lowers the damage); debug field `lastAmount` as stock. Same code in STRUT (`0x6a3840`) and ML (`0x6a36c0`). |
| `Suspension::addForceAtPos` | 0x1402c0bf0 | 0x1123000 (0x26ad110) | **C, unconditional** | Applies the force to the hub body (vtable +0x110) and, when the steer-torque flag is set, adds `((P - centre) x F) . axis` **multiplied by `abs(u . F) / abs(F)`** with `u` = unit vector from the body's steer pick-up point to the hub's (`+0x15c` / `+0xec`), i.e. the share of the force along the tie rod; `1.0` when `abs(F)^2 <= 1.2e-7`. No ext test: applies to every DWB wheel [dll]. Effect: longitudinal forces (perpendicular to the rod) contribute less steer torque / FFB than in stock AC [interp]. |
| `Suspension::step` | 0x1402c3390 | **0x11232b0** (0x268c410) | B (+ own PID) | 4,808 B wrapper (not decompiled): references the global byte `0x2695085`, the ext lookup, the original through the slot, `PIDController::eval` (original, slot 0x268b418: AC's dead "active actuator" path), the damper-LUT / bump-stop code of the DWB2 area (`0x8328c0` x5, `0x833ed0`, `0x852160`, `0x8af330`). [dll] |
| `SuspensionStrut::SuspensionStrut` | 0x1402c38d0 | 0x6a3770 (0x26a7068) | B | Original, tag 2, `+0x34 := 0`. |
| `SuspensionStrut::attach` | 0x1402c4100 | 0x6a37a0 (0x26a7070) | B | Original, tag 2, extra-links builder (see above). |
| `SuspensionStrut::setDamage` | 0x1402c6160 | 0x6a3840 (0x26a7078) | C with ext | As DWB, strut layout (`+0x1c4..+0x1dc`). |
| `SuspensionStrut::step` | 0x1402c6600 | 0x6a3a50 (0x26a7080) | B | Profiler scope; **sets TLS `+0x228 := &damper (this+0x168)`** and jumps to the original (so `Damper::getForce` knows it is called for a strut); with byte `0x2695085` set and tag 1/2, `physicsGUID < 256`: hash of five strut / steer points (golden-ratio combine) compared with a per-(car, wheel) record (0x160 bytes at `0x254a990`); when it changed a CSP routine recomputes derived strut data [interp]. |
| `SuspensionStrut::getSteerBasis` | 0x1402c4d70 | **0x6a3900** (0x268b620) | B | With ext byte `+0xb7 == 1` (or `+0xb7 == 0xFF` and global byte `0x26a2c04`): the strut point at `+0x94` is temporarily replaced by the vector at `+0xb8` while the original computes centre / axis, then restored: **`FIX_STRUT_FFB`** (string in the loader `0x7f8ae0`). [dll][interp for which point] |
| `SuspensionAxle::SuspensionAxle` | 0x1402c6b90 | 0x6a33d0 (0x26a7038) | B | Original, tag 3. |
| `SuspensionAxle::attach` | 0x1402c7fb0 | 0x6a3400 (0x26a7040) | B | Original, tag 3, extra-links builder. |
| `SuspensionAxle::step` | 0x1402c8770 | 0x6a3490 (0x26a7048) | A/B | Profiler scope around the original. |
| `SuspensionML::SuspensionML` | 0x1402c8e20 | 0x6a3600 (0x26a7050) | B | Original, tag 2 (see common note). |
| `SuspensionML::attach` | 0x1402c9420 | 0x6a3630 (0x26a7058) | B | Original, tag 4, extra-links builder. |
| `SuspensionML::setDamage` | 0x1402ca980 | 0x6a36c0 (0x26a7060) | C with ext | As DWB, ML layout (`+0x9c..+0xb4`). |
| `SuspensionAnimator::addModel` | 0x1401b14f0 | **0x6a3320** (0x268b610) | A/B | Original, then stores `this` in the car-info record being loaded (`*_DAT_1826a9d50 + 0x50`) if empty and tests the settings byte `+3` (graphics side). |
| lookups | | | D | `SuspensionAnimator::SuspensionAnimator` 0x1401b0770 (0x268b608), `suspensionAnimatorUpdate` 0x1401b25a0 (0x268b600), `SuspensionAvatar::SuspensionAvatar` 0x1401b2fe0 (0x268b618): called by CSP's car-avatar builder `0x32fe80` ("Animated suspension (LOD)" / "Non-animated suspension") and the update caller `0x125a9b0`. |

### 2.5 Chassis springs and dampers

| Symbol | acs VA | CSP RVA (slot) | Cat | What it changes |
|---|---|---|---|---|
| `Damper::getForce` | 0x1402b3280 | 0x5a7210 (0x26a4860, never called) | C | Same bump / rebound formula as stock, evaluated at `-v` when the damper equals the thread-local marker `TLS+0x228` (set by the STRUT step wrapper to the strut's own damper). Unconditional. See open question 3 for the sign. |
| `HeaveSpring::step` | 0x1402b3960 | 0x5eb720 (0x268b170) | B | With ext: profiler scope (+-3.0 id for front / rear), then the **DWB2 object** (`ext+0x1b8`, when present) updates three float members of the heave spring (`+0x0c, +0x10, +0x18`: progressive rate / rod length / bump-stop rate by the layout of `docs/map/suspension.md`) through `0x8b0240` (4,026 B), the original steps, the three members are restored. |
| `AntirollBar::step` | 0x1402bb640 | 0x58be00 (0x268a980) | B | Profiler scope, original, then **for `k < 0`** (the original does nothing for `k <= 0`) the same push-pull force on the two hubs and the body (`n * delta * k`, `n` = body row 1, not renormalised) is applied. Negative rates therefore work (rate from a `ctrl_arb_*.ini` controller or script). |
| `BrushSlipProvider::recomputeMaximum` | 0x1402b3230 | lookup (0x268a9d0) | D | No CSP caller (only its registration stub). |

### 2.6 CSP-own functions that implement the features (not table rows)

Sizes from the `.pdata` chunks; strings from `dll_func_strings.tsv`. None of these is decompiled.

| RVA (size) | What it is (from strings / call graph) |
|---|---|
| 0x69c4f0 (+0x69c6fe) 526 + 3,730 B | CSP's V10 tyre-model solver, called by the `SCTM::solve` hook and directly by `0x6b4170`; no strings. |
| 0x6b4170 10,040 B | CSP `addTyreForcesV10` body. |
| 0x6b2e90 4,532 B | CSP `addGroundContact` body (calls the original). |
| 0x6b1950 2,411 B | batched ray executor; string "Car #1/Tyre/Precise sine noise". |
| 0x6a9b60 1,115 B | `Tyre::Tyre` body; string "Using linear interpolation on tire load sensitivity curves"; key `SMOOTH_LOAD_SENS` is the next pool string. |
| 0x6aadc0 304 B, 0x6aac60 345 B | surface-noise height / normal helpers used by `Tyre::step`. |
| 0x8cc0e0 30,533 B | CSP's own `tyres.ini` compound loader: "Loading tire compound #", reads `[FRONT*]`, `SHORT_NAME`, `OVERRIDE_BLANKETS_TEMP`, `[ADDITIONAL1] BLANKETS_TEMP`. |
| 0x8d3ad0 1,999 B / 0x8d38f0 479 B | falloff (`FALLOFF_LEVEL/SPEED`, "Tire constant falloff found: 0.8", "falloff curve") and camber-curve readers. |
| 0x917c30 4,170 B / 0x91cfb0 4,895 B / 0x91e2d0 / 0x919d00 4,274 B | tyre type classification (`TYPE_HINT`, "type determined as SLICK ..."), "Rain Extension: Found rain tire / Assuming slick tire", virtual rain tyre generator. |
| 0x922b60, 0x923a10 | ice compounds ("Ice Rally", "Ice Street", `ICE-R`, `ICE-S`, "Creating ice tyres from"). |
| 0x91b410 / 0x91b660 578 B each | `THERMAL_FRONT(_n)`, `THERMAL_REAR(_n)` and `THERMAL2_*` section readers. |
| 0x8f2240 14,505 B, 0x8fc860, 0x9004c0 | layered thermal model parameter table / debug UI (33 names: "Bulk Roll Resist", "Core to SW Outer", "Surface Road Friction", "Rim to Liner Radiation", ...; "Thermal Model V4: Base layer weights"). |
| 0x8d4470 1,266 B, 0x8d6a60 3,183 B, 0x8d42a0 352 B, 0x8d76d0 214 B, 0x8f83d0 555 B | TM (per-car tyre system) functions seen from the hooks. |
| 0x7f8ae0 5,693 B | car loader piece reading `assists.ini [ASSISTS] _EXT_RAIN_PHYSICS`, `suspensions.ini [_EXTENSION] FIX_PROGRESSIVE_RATE, DEBUG_MODE, FIX_STRUT_FFB, TORQUE_MODE_EX`, `engine.ini [_EXTENSION] EXT_GAS_CURVE, EXT_SPIN_DELAY`, `drivetrain.ini [_EXTENSION_AUTOCLUTCH] AWD2_INVERTED`, `PYTHON_TORQUE_ALLOWED`; the pool around it holds `RIM_SPRING_RATE` / `RIM_DAMPING_RATE`. |
| 0x87f080 60,194 B | **COSMIC suspensions.ini loader** (123 strings, see 2.7). |
| 0x87e090 848 B, 0x87e3e0 3,180 B | "Creating cosmic suspension for wheel", "Car suspension initiated in wrong order, unable to get bodies", body lookup (`HUB_L`, `HUB_R`, `CHASSIS`, mirror). |
| 0x88e600 1,048 B, 0x88f380 11,118 B, 0x88eaf0 2,179 B | joint ERP/CFM and creation / attach ("Cosmic wheel", "Attaching wheel body to cosmic suspension"). |
| 0x897ec0 15,374 B | per-step COSMIC update; profiler scope "Car #1/Cosmic". |
| 0x89c570 10,672 B | setup.ini item bindings of COSMIC (`DAMPER_BUMP_<w>`, `SPRING_RATE_`, `TORSION_RATE_`, `ZERO_LENGTH_OFFSET_`, `BUMPSTOP_NUM_/GAP_`, `..._POS_A_SETTING_`, `STEER_JOINT_n_LENGTH_OFFSET_`). |
| 0x8a9c80 15,178 B, 0x8b1200 4,989 B, 0x8b0240 4,026 B | DWB2: "Using AC Patch Custom DWB Suspension", "Using new / old bumpstop model", `bumpstops.ini`, `SPRING_LF/RF/LR/RR` found in setup.ini, `BUMPSTOP_<corner>`, `FRONT_GEOMETRY`, `REAR_GEOMETRY`. |
| 0x82d5c0 16,681 B, 0x831840, 0x8328c0, 0x8342f0 | `dampers.ini` ("Loading damper hysteresis params"; `BUMP_SLOW_n`, `BUMP_FAST_n`, `REBOUND_*`, `FRONT_HEAVE`, `REAR_HEAVE`, `DAMPER_BLOWOFF_`). |
| 0x8bb980 9,162 B | Virtual K&C tests: heave, roll, lateral opposed / parallel, longitudinal opposed / parallel, Mz opposed / parallel (8 tests; CSV header "Time (s), FX (N), FY (N), FZ (N), Tire Load (N), Tire Load Sum (N), Mz (Nm), Camber (deg), Toe (deg)"). |
| 0x84c650 6,031 B, 0x84b5f0 938 B, 0x84b290, 0x84f430 7,200 B | `_EXTRA_LINK`, `_AC_LINK_ALTERATION` / `PRINT_OUT`, joint print ("distance", "fixed", "slider", "rigidity", "CFM"), "Creating cphys ARB". |

### 2.7 Answers to the four questions

**(1) What is the COSMIC suspension model.**

* It is **not** one of AC's four types: acs.exe's `Car::Car` compares `TYPE` with STRUT / DWB / ML / AXLE only and exits on anything else (`docs/map/suspension.md` 4). CSP maps the car onto `Suspension` objects, and the `Suspension::Suspension` hook builds a CSP object (RTTI `cphys_suspension_cosmic`, tag 5) when a factory is registered for the wheel [dll][interp].
* **Building blocks** (from the log strings of `0x88f380` and the key pool of `0x87f080`): 6 joint kinds - ball (`J<n>` / `BALL_JOINT_`), distance (`DJ<n>` / `DISTANCE_JOINT_`), hinge (`HJ<n>` / `HINGE_JOINT_`, built from two sub-joints), steering link (`STEER_JOINT_<n>`, distance joint + motor), slider (`SLIDER_<n>`), rack (`RACK_*`) - plus one composite, the compliant **wishbone** (`WISHBONE_<n>`: `K_FRONT`, `K_REAR`, `K_HUB`, `K_AXIAL_F/R`, `D_AXIAL`, `D_ORTH`, `POS_FRONT`, `POS_REAR`, `POS_HUB`: bushing stiffness / damping); 4 force elements - spring (`_SPRING_<n>`: `SPRING_LUT`, `CALIBRATION_LUT`, `RATE_MULT`), coil-over (`_COILOVER_<n>`: `INSTALL_RATE`, `RATE`, `PRELOAD(_FORCE)`, `MIN/MAX_LENGTH`, `END_RATE/END_VTAPER/END_VMAX`, `BUMPSTOP_GAP/LUT`, `HELPER_RATE/TRAVEL`, `COMPENSATE_MASS`, `MASS_INNER/OUTER`, `INSTALL_DAMP`, `FLIP_PRELOAD`), torsion bar (`_TORSION_<n>`: `RATE`, `PRELOAD_TORQUE`, `HINGE`, `PARITY`), damper (`_DAMPER_<n>` + `dampers.ini` LUTs, `USE_ADVANCED`, blow-off); plus the anti-roll mechanism ("Creating cphys ARB", `[ARB] EXTEND`, `USE_FOR_ARB`, `FRONT/REAR_MOTION_RATIO`). Bodies are named (`BODY_<n>_NAME`, `_MASS`, `_INERTIA`, `_POS`, `_CARPOS`, `_MIRROR`, `IS_CHASSIS_BODY`, `HUB_MASS`, `HUB_INERTIA`, special names `HUB_L`, `HUB_R`, `CHASSIS`); the **wheel is a body of its own** ("Attaching wheel body to cosmic suspension", "Cosmic wheel") spun by the tyre's torque and read back by `updateAngularSpeed`.
* **Per-joint softness and friction**: `OVERRIDE_ERPCFM`, `ERP/CFM` log lines, joint motors ("Ball / Distance / Steering joint motor creation failed"), `SPRING_K`, `DAMP_K`, `FRICTION_C/K`, `GLOBAL_FRICTION_C/K` [interp: motors implement joint friction and compliance].
* **Setup coupling**: positions, lengths and spring parameters are bound to `setup.ini` items by name, so a setup change moves joints (push/pull-rod length, toe, steering ratio, ballast) or changes rates.
* **Diagnostics**: 8 virtual K&C tests (0x8bb980), `ac.PhysicsDebugLines.CosmicBodies`, `[_AC_LINK_ALTERATION] PRINT_OUT=1`.
* **Differences against acs.exe**:

| | DWB | STRUT | AXLE | ML | COSMIC |
|---|---|---|---|---|---|
| bodies | hub | hub + strut body (80 / 20 % of `HUB_MASS`) | 1 shared axle | hub | any number of named bodies + wheel body |
| links | 5 distance joints, fixed ERP 0.3 / CFM 1e-7 | 3 distance + slider + ball | `LINK_COUNT` distance | 5 distance | ball, distance, hinge, steering, slider, rack, wishbone (compliant); per-joint ERP/CFM |
| spring / damper | analytic, along body "up", wheel rate, no motion ratio | analytic along the strut (damper sign swapped) | analytic along axle-body | analytic, no bump stops | physical force elements between bodies (motion ratio emerges), LUTs and hysteresis |
| anti-roll | `AntirollBar` by hub height difference | same | same | same | mechanism (U-bar, drop links) or the same bar |
| steering | rod offset `setSteerLengthOffset` | same | none | same | steering joint / rack + `TOEVECTOR`, `STEERVECTOR`, `ADD_TO_FFB` |
| wheel spin | tyre integrates | same | same | same | wheel rigid body |
| setup | members via `SetupManager` | same | same | same | `setup.ini` items by key name |

**(2) Does CSP replace the tyre model or only add?** Per hook, for a car that has the per-car object (all cars [interp]):

| Part | Verdict |
|---|---|
| `Tyre::step` | **replaced** (original body never runs) by a CSP body that re-implements the flow and calls selected originals by slot. |
| `Tyre::addTyreForces` (VERSION < 10, brush model, kart `formula_k` and the 130 older tyre sets) | **kept**: original code, called from CSP's step; inputs changed by `getCorrectedD` (no pressure term), `getDY/getDX` (clamped / optional linear curve), `stepDirtyLevel` post-processing. |
| `Tyre::addTyreForcesV10` and `SCTM::solve` (VERSION >= 10, every modern car) | **replaced** by CSP code (0x6b4170, 0x69c4f0). Stock-equivalence unproven. The SCTM hook uses CSP's solver when the TLS "current tyre" is set and the car has the TM; otherwise the original. |
| `getDY`, `getDX` | replaced; same formulas, differences listed in 2.1. |
| `getCorrectedD` (Tyre) | replaced; drops the pressure divisor. |
| `updateAngularSpeed`, `updateLockedState` | replaced; stock-equivalent except COSMIC. |
| `addGroundContact` | extended (original is called). |
| `stepDirtyLevel` | extended (post-processing). |
| `stepThermalModel`, `TyreThermalModel::*` | version 0: original + extras; versions >= 2: replaced by layered model. |
| `stepGrainBlister`, `stepFlatSpot`, `stepPuncture`, `stepRotationMatrix`, `getDynamicK` | **untouched** (called through slots). |
| `Tyre::reset`, `setCompound`, `initCompounds`, `stepTyreBlankets`, `~Tyre` | replaced / extended as in 2.1. |
| `Curve::getCubicSplineValue`, `Curve::getValue`, `Curve::load` (group rendering_hooks, but they feed the tyre curves) | cubic spline clamps `x` to the first / last reference and returns 0 for an empty curve (stock extrapolates the polynomial); `getValue` multiplies the result by a thread-local factor when the curve carries marker byte `0xD5`; `load` accepts inline curve names that start with `__INLINED:(` followed by `x=y` pairs separated by pipe characters (CSP generates such names itself, e.g. pairs 20=1, 40=1.0, 80=0.97, 160=0.9, 250=0.83 and 0=0.98, 0.3=1, 1.0=0.9). Affects every LUT in the game. |

**(3) What changes at which step.** Tyre: 2.2 (phases 1-21). Suspension, per 3 ms step inside `Car::stepComponents` (the suspension loop is a vtable call on tag-specific code): DWB -> `0x11232b0` (wrapper: DWB2 / PID / LUT code, else original); STRUT -> `0x6a3a50` (marker + optional geometry cache, then original); AXLE -> scope + original; ML -> not hooked; COSMIC -> CSP class (`0x897ec0`, "Car #1/Cosmic"). Then HeaveSpring (DWB2 update, original, restore), AntirollBar (original + negative-k branch). Outside the step: `attach` (extra links, once at the 4th wheel), `setDamage` (monotonic damage), `getHubWorldMatrix` (extra camber), `addForceAtPos` (tie-rod factor on steer torque), `getSteerBasis` (strut FFB fix).

**(4) Data the tyre model exports to Lua** (all from the SDK, `ac_car_cphys/lib.lua`):

* `ac.accessCarPhysics().wheels[0..3]` = `ac.StateCphysWheel`, **36 fields**: `angularSpeed`, `coreTemperature`, `insideTemperature`, `middleTemperature`, `outsideTemperature`, `slip`, `slipAngle`, `slipRatio`, `dynamicPressure`, `staticPressure`, `ndSlip`, `load`, `depth`, `abs`, `dx`, `dy`, `fx`, `fy`, `mz`, `inflation`, `suspensionTravel`, `suspensionDamage`, `damperSpeed`, `surfaceGrip`, `surfacePitlane`, `surfaceValidTrack`, `surfaceExtendedType`, `transform`, `side`, `up`, `look`, `position`, `velocity`, `angularVelocity` (hub, without wheel spin), `contactPoint`, `contactNormal`.
* `ac.getCarWheelSurface(i)` = `ac.StateCphysSurface`, 13 fields (grip, sectorID, dirt, collisionCategory, isValidTrack, blackFlagTime, sinHeight, sinLength, isPitlane, damping, granularity, vibrationGain, vibrationLength); `ac.accessCarDamper(i)` = `ac.StateCphysDamper`, 6 read-write fields (reboundSlow, reboundFast, bumpSlow, bumpFast, fastThresholdBump, fastThresholdRebound).
* Graphics-thread `ac.getCar(i).physics.wheels[]` = `ac.StateWheelPhysics`, 11 fields (driveTorque, brakeTorque, handbrakeTorque, electricTorque, feedbackTorque, angularInertia, shaftVelocity, shaftInertia, tyreCarcassTemperature "only with custom physics tyres", roadTemperature, discCoreTemperature); `ac.StateWheel` has 63 fields including `tyreRadius`, `tyreWidth`, `tyreDirty`, `tyreWear`, `tyreVirtualKM`, `tyreGrain`, `tyreBlister`, `tyreFlatSpot`, `tyreStaticPressure`, `tyrePressure`, `tyreCoreTemperature`, `tyreInside/Middle/OutsideTemperature`, `tyreOptimumTemperature`, `load`, `speedDifference`, `camber`, `toeIn`, `suspensionDamage`, `suspensionTravel`, `tyreLoadedRadius`, surface* fields.
* Writes: `ac.CarPhysicsValueID` 10001..10005 (`ForcelessTyres` = flag bit 1, `TyresVirtualKm`, `TyresDirtLevel`, `TyresFlatSpot`, `TyresSlipRatio`), `ac.setTyreInflation`, `ac.setTyreWearMultiplier`, `ac.shiftPatchTemperatures`, `ac.setTyresBlankets`, `ac.addWheelsFeedbackTorque`, `ac.addHubForce(2)`, `ac.addTorque(hubIndex)`, `ac.addCosmicForce/Torque`, `ac.addSuspensionDamage`.
* The native side also fills the **value block** at `0x7381c0` (2.2 phase 20) for the graphics / audio side.

## 3. Config keys and Lua API

### 3.1 Keys CSP reads (CSP-only), and which CSP function reads them

Stock keys the CSP code still needs are in `docs/map/tyre_ini_keys.md` and `docs/map/suspension.md`. "no string" = the key name does not exist as ASCII or UTF-16 text anywhere in the 139,567,496-byte file (checked with a full-file search); the reader is therefore addressed by hash or by an obfuscated table that I did **not** try to decode (rule 2); those keys are known only from mod data ([prior] `alpha_keys_csp_tyres.txt`, `physics_documented.md` 3.2).

**`tyres.ini`** (of the 61 CSP-only names in the 2026 VRC car's file: 9 have text in the DLL, 52 do not)

| Key(s) | Where | Read by |
|---|---|---|
| `[HEADER] VERSION`, `FALLOFF_LEVEL`, `FALLOFF_SPEED`, `DCAMBER_LUT` (stock keys, CSP re-reads) | per compound | 0x8d3ad0 / 0x8d38f0 ("Read tyres.ini ... correctly") |
| `SHORT_NAME`, `OVERRIDE_BLANKETS_TEMP`, `[ADDITIONAL1] BLANKETS_TEMP` | per compound | 0x8cc0e0 |
| `TYPE_HINT` (SLICK, SEMISLICK, TREADED, RACING_TREADED, VINTAGE_PERFORMANCE), `TYPE_HINT_MODIFIER` | per compound | 0x917c30 |
| `TREAD_COVER`, `TREAD_DEPTH`, `TREAD_CLEAR_K`, `GRIP_MULT`, `RELAXATION_LENGTH`, `FRICTION_LIMIT_ANGLE`, `ROLLING_K`, `PERFORMANCE_CURVE`, `_VIRTUAL_ORIGIN` | generated sections | rain / ice generators 0x919d00, 0x91cfb0, 0x922b60, 0x923a10 [interp] |
| `THERMAL_FRONT(_n)`, `THERMAL_REAR(_n)`, `THERMAL2_FRONT(_n)`, `THERMAL2_REAR(_n)` | section names | 0x91b410, 0x91b660 |
| `RIM_SPRING_RATE`, `RIM_DAMPING_RATE`, `SMOOTH_LOAD_SENS` | `[_EXTENSION]` [interp] | 0x7f8ae0 neighbourhood; `SMOOTH_LOAD_SENS` -> `Tyre::Tyre` body 0x6a9b60 |
| `PERFORMANCE_MAP` (2D lut) | `[THERMAL_*]` | text at 0x2245f98, no function reference found (base-register addressing; layered-model loader 0x8f2240 area) |
| `EXTENDED_RAYTRACING`, `LATERAL_RAYS` (+0x74c), `LONGITUDINAL_RAYS` (+0x750), `MAX_RAY_ANGLE` (+0x754), `MAX_RAY_WIDTH_RATIO`, `DISABLE_RAY_DOUBLING` (+0x7a8), `PRACTICAL_TEMP_RATIO`, `USE_NEW_RR_SLIP`, `ENABLE_RAY_FORCE_WEIGHTING` (+0x7a9), `USE_NEW_VKM`, `SLIP_VERSION`, `USE_TREAD_COVER` (14 `[_EXTENSION]` keys) | `[_EXTENSION]` | no string; the offsets in parentheses are the TM fields `Tyre::step` reads |
| `RADIUS_ANGULAR_K_2`, `DX_CAMBER_REF`, `DX_CAMBER_MULT`, `ROLLING_RADIUS_MULT`, `CAMBER_SPRING_MULT`, `FALLOFF_LEVEL_CURVE`, `FALLOFF_SPEED_CURVE`, `FALLOFF_YSPEED_MULT`, `PRESSURE_GROWTH_GAIN`, `SIDEWALL_LOSS_FACTOR_Z`, `SIDEWALL_K_MULT(_X)`, `WEAR_RATE`, `SAT_PEAK_K`, `SAT_MULT`, `SAT_MIN_REF`, `SAT_LOAD_K`, `SAT_MULT_K`, `SAT_SPEED`, `COMBINED_FACTOR_1`, `COMBINED_FACTOR_BRAKE_MULT`, `COMBINED_FACTOR_LOAD_K`, `DROPOFF_FACTOR_0/1` | per compound | no string |
| `OPTIMAL_TEMP_OFFSET`, `BULK_THICKNESS`, `SIDEWALL_THICKNESS`, `VOLUMETRIC_HEAT_CAPACITY`, `CONDUCTIVITY`, `EFFUSIVITY`, `WEAR_K`, `SURFACE_TO_AMBIENT(_SIDEWALL)`, `ENGINE_BAY_MIX`, `RADIATION_CONSTANT`, `CARCASS_ROLLING_K`, `FLEX_SPEED_COEFF`, `INTERNAL_CONVECTION_K`, `RIM_TO_AMBIENT`, `BRAKE_TO_RIM` | `[THERMAL_*]` | no string |
| `GENERATE_EXTRA_TYRES` | `surfaces_fx.ini`/launcher [interp] | text at 0x2246c70, no function reference |
| `[THERMAL_MODEL] VERSION` (0..4) | tyres.ini | the TM `+0x238` value; thresholds in 2.3 |

Rain-physics assist: `assists.ini [ASSISTS] _EXT_RAIN_PHYSICS` (default 1) -> 0x7f8ae0 -> `ext+0xb4`.

**`suspensions.ini`** (CSP-only; stock keys `TYPE`, `BASEY`, `TRACK`, `RIM_OFFSET`, `HUB_MASS`, `STATIC_CAMBER`, `TOE_OUT`, `[BASIC] WHEELBASE`, `CG_LOCATION`, `[DAMAGE] MIN_VELOCITY/MAX_DAMAGE/DEBUG_LOG`, `[ARB]` are re-read by 0x87f080)

| Key(s) | Read by |
|---|---|
| `TYPE=COSMIC` | 0x87f080; `Car::Car` mapping in `0x595b70` |
| `BODY_<n>_NAME/_MASS/_INERTIA/_POS/_CARPOS/_MIRROR`, `IS_CHASSIS_BODY`, `USE_FOR_ARB`, `HUB_MASS`, `HUB_INERTIA`, `ANGULAR_INERTIA`, `BASE_R_OFFSET`, `ENGINE_TORQUE_BODY/HUB_POS/CHASSIS_POS/..._CARPOS` | 0x87f080 |
| joints `J<n>`, `HJ<n>`, `DJ<n>`, `STEER_JOINT_<n>`, `SLIDER_<n>`, `WISHBONE_<n>`, `BALL_JOINT_`, `HINGE_JOINT_`, `DISTANCE_JOINT_`, each with `_BODY_A/_BODY_B`, `_POS_A/_POS_B`, `_CARPOS_A/_B`, `_CARPOS0/1`, `_KP`, `PARITY`, `_LENGTH_OFFSET`, `ZERO_LENGTH_OFFSET`, `PULL_FORCE`, `ELECT_LOG`, `OVERRIDE_ERPCFM`, `FFB_LOW_PASS_HZ`, `TOEVECTOR`, `STEERVECTOR`, `ADD_TO_FFB`, `RACK_*` | 0x87f080 (creation 0x88f380, ERP/CFM 0x88e600) |
| `[<AXLE>_TORSION_n]`, `[..._SPRING_n]`, `[..._COILOVER_n]`, `[..._DAMPER_n]` and their keys listed in 2.7 | 0x87f080 |
| `[ARB] EXTEND`, `FRONT_MOTION_RATIO`, `REAR_MOTION_RATIO` | 0x84f430 "Creating cphys ARB" / 0x87f080 |
| `[_EXTENSION] FIX_PROGRESSIVE_RATE`, `FIX_STRUT_FFB`, `DEBUG_MODE`, `TORQUE_MODE_EX` | 0x7f8ae0 |
| `[_EXTENSION] USE_DWB2`, `DAMPER_LUTS`, `SEPARATE_BSH_GAPS` | `DAMPER_LUTS` has text (one reference, dampers area); `USE_DWB2`, `SEPARATE_BSH_GAPS` no string; DWB2 selection log is in 0x8a9c80 |
| `[_EXTRA_LINK_n]` `FROM_POS`, `TO_POS`, `ANCHOR`, `RIGIDITY`, `FIXED`, `DISTANCE`, `LINKS`, `DEACTIVATE`, `WHEELS` (`LEFT_FRONT`, `RIGHT_FRONT`, `LEFT_REAR`, `RIGHT_REAR`), `DEBUG_COLOR`; `[_AC_LINK_ALTERATION] PRINT_OUT` | 0x84c650, 0x84b5f0 |
| `FRONT_GEOMETRY`, `REAR_GEOMETRY` (K&C reference) | 0x8b1200 / 0x8bb980 |
| `[_EXTENSION_FLEX]` chassis flex | function 0x811810 (other brief) |

**`dampers.ini`, `bumpstops.ini`, `setup.ini`**: read by 0x82d5c0 / 0x831840 / 0x8328c0 / 0x8342f0 (dampers), 0x8a9c80 / 0x8b1200 (bump stops, `SPRING_*` entries), 0x89c570 (setup bindings). Key names in 2.6 and `physics_documented.md` 3.2.3-3.2.4.

### 3.2 Global option bytes seen in these hooks (names by behaviour, [interp])

| Byte (RVA) | Behaviour | Likely key |
|---|---|---|
| `0x2695085` | gates the STRUT geometry-hash cache and a block of the DWB step wrapper | hidden `[PHYSICS_EXPERIMENTS_DONE] SLIDING_FIX` (file order) |
| `0x2695086` | brake-torque limit in the wheel torque sum | `BRAKES_LIMIT` |
| `0x2695091` | random initial wheel angle | `RANDOMIZE_WHEEL_ANGLES` |
| `0x2695094` | surface noise shifts the ray origin | `TYRE_SHIFT_SURFACE_PROPERLY` |
| `0x269507c`, `0x269507f` | "custom raycasting" on / mode (2 = Embree) | `ODE_BATCH_RAYCAST` / custom raycasting |
| `0x26950a6` | extra contact-point probe branch | `EXTRA_CONTACT_POINTS` |
| `0x26a2c04` | default of `FIX_STRUT_FFB` | (per-car key overrides) |
| `0x1826b8a18` (`DAT_1826b8a18`) | disables multi-ray when set | unknown (also read by other hooks) |

These bytes are **not** in file order of `general.ini` (so the mapping of `physics_documented.md` 2.4 by order is not safe).

### 3.3 Lua API (car physics scripts, `ac_car_cphys`)

* Tyre / wheel: `ac.setTyreInflation`, `ac.setTyreWearMultiplier`, `ac.shiftPatchTemperatures`, `ac.setTyresBlankets`, `ac.addWheelsFeedbackTorque`, `ac.getWheelsInertia`, `ac.overrideTyreSmoke`, `ac.overrideTyreIgnitionPoint` (8); `ac.overrideSpecificValue(id, value, tyres)` with `ac.CarPhysicsValueID.ForcelessTyres / TyresVirtualKm / TyresDirtLevel / TyresFlatSpot / TyresSlipRatio` (1 function, 5 tyre ids).
* Forces / suspension: `ac.addHubForce`, `ac.addHubForce2`, `ac.addTorque` (with `hubIndex`), `ac.addCosmicForce`, `ac.addCosmicTorque`, `ac.addSuspensionDamage`, `ac.suspensionJointConfigure`, `ac.getSuspensionJointPointTo`, `ac.suspensionJointReseat`, `ac.suspensionJointReseatDistance`, `ac.accessCarDamper`, `ac.getCarWheelSurface`, `physics.getExtendedSuspensionTravel`, `physics.getExtendedDamperTravel` (14).
* Total for car scripts: 23 (21 `ac.*`, 2 `physics.*`).
* Track / app / tool / online scripts (not car scripts), `physics.*` tyre and suspension functions: `setTyresTemperature`, `setTyresVirtualKM`, `setTyresCompound`, `setTyresBlankets`, `setTyresAIMultiplier`, `blowTyres`, `setSuspensionDamage`, `addWheelTorque`, `setWheelAngularVelocity`, `setAITyres`, `setAITyresHint`, `setAIRainTyres`, `setAllowedTyresOut` (13).
* State structs: `ac.StateCphysWheel` 36 fields, `ac.StateCphysDamper` 6, `ac.StateCphysSurface` 13, `ac.StateWheelPhysics` 11, `ac.StateWheel` 63.
* Debug: `ac.PhysicsDebugLines.Tyres` (raycasts), `.WetSkidmarks`, `.CosmicBodies`.

## 4. Files it reads

Car data: `tyres.ini` (also through `ac.INIConfig.carData(i, 'tyres.ini')`, which returns the **modified** in-memory file with generated compounds), `suspensions.ini`, `dampers.ini`, `bumpstops.ini`, `setup.ini`, `car.ini`, `assists.ini`, `engine.ini`, `drivetrain.ini`, LUT files named by those (`tyres_*_T_*.2Dlut`, `damper_*.lut`, `bumpstop_*.lut`, `setup_*.lut`); `extension/config/general.ini` `[PHYSICS_EXPERIMENTS*]`, `rain_fx.ini [BASIC] AUTOSELECT_RAIN_TYRES`, `data_apps_virtual_tyres_fix.txt` (apps that get the original tyre name instead of the virtual one); per-car `extension/ext_config.ini` and `config/cars/*` for looks only; `surfaces.ini` through the track (grip, sin noise). [dll][ini]

## 5. How it is switched on

* Tyre code: on for every car that has the per-car object (all cars [interp]); the Embree path unless `ODE_BATCH_RAYCAST=1`; multi-ray only for ext index 0 and only with non-zero `[_EXTENSION] LATERAL_RAYS / LONGITUDINAL_RAYS`; layered thermal model by `[THERMAL_MODEL] VERSION` and compound keys; wet-grip law by `_EXT_RAIN_PHYSICS`; gas-law pressure always; brake limit and random wheel angles by hidden always-on options.
* COSMIC: `suspensions.ini TYPE=COSMIC` (+ an installed CSP >= the car's `[_EXTENSION] REQUIRED_VERSION`); DWB2: `[_EXTENSION] USE_DWB2=1`; extra links / alteration: sections present in `suspensions.ini` (any suspension type); strut FFB fix: `FIX_STRUT_FFB=1` or the global default.
* Extended physics (`VERSION=extended-N`) is not required for these hooks to run, but the keys that matter live in files only extended cars ship (`physics_documented.md` 5.1).
* On this PC (F2004, Spa): gas-law pressure, brake limit, Embree, wet-grip hook, virtual rain compound, strut/DWB hooks active; no COSMIC car is driven; 3 installed COSMIC cars (`vrc_formula_alpha_2025_csp`, `..._2026_csp`, `..._beta_2024_csp`), 1 DWB2 car (`vrc_formula_lithium_2023_csp`) [prior].

## 6. Size

| Item | Number |
|---|---|
| Table rows | 60 (43 hooks + 17 lookups), corrected to 47 hooks + 13 lookups |
| Hook entry functions decompiled | 43 files, 212,324 bytes of pseudo-C (`Tyre` 152,494 in 17 files, `TyreThermalModel` 16,190 in 8, `Suspension` 12,105 in 5, `SuspensionStrut` 9,210 in 4, `SuspensionAxle` 7,503 in 3, `SuspensionML` 7,191 in 3, `AntirollBar` 3,739, `HeaveSpring` 2,700, `Damper` 1,192) |
| `Tyre::step` | 22,180 bytes of machine code (0x6ac2a0..0x6b1944), 2,544 pseudo-C lines |
| CSP-own tyre-path callees not decompiled | 0x6b4170 10,040 B; 0x6b2e90 4,532 B; 0x69c4f0+0x69c6fe about 4,250 B; 0x6b1950 2,411 B; 0x6a9b60 1,115 B; 0x8cc0e0 30,533 B |
| COSMIC / DWB2 / link code (not decompiled) | 0x87f080 60,194 B; 0x897ec0 15,374 B; 0x88f380 11,118 B; 0x89c570 10,672 B; 0x8a9c80 15,178 B; 0x11232b0 4,808 B; 0x82d5c0 16,681 B; 0x8bb980 9,162 B; 0x84c650 6,031 B |
| Thermal V4 | 0x8f2240 14,505 B and neighbours; 33 named parameters |
| Lua | 23 car-script functions (21 `ac.*`, 2 `physics.*`), 13 track-side `physics.*`, 5 state structs (36 + 6 + 13 + 11 + 63 fields) |
| Keys | 61 CSP-only tyres.ini names tested (9 with text, 52 without); 123 distinct strings in the COSMIC loader |
| Virtual compounds | F2004 under CSP: 4 compounds (3 + rain) [prior]; the 2026-10-12 CSP log (`rss_formula_hybrid_2020`) prints 7 type classifications and "Loading tire compound" #0..#5 |

## 7. Port difficulty for rustyAC

Overall **XL** for "tyres and suspension as CSP does them"; but the part that matters for a stock car on a stock track is **S to M** and can be done first.

| Item | Size | Reuse / proof |
|---|---|---|
| Gas-law pressure | S | already an option (`--pressure-law`, `docs/port/grip_investigation.md`); replace the fixed `0.16/C` law by the formula of 1.2 using the altitude pressure function (`0xaf75f0`, same family as the air-density law already ported). Prove with `f2004_spa_ai.csv` / the owner's recording: 4 wheels within 0.03 psi. |
| Brake limit clamp, random initial wheel angle, damage = max(old, new), negative-k ARB, cubic-spline clamping, `getDY/getDX` differences | S each | `vanilla_tyre.rs`, `antiroll_bar.rs`, `suspension.rs`, `curve.rs`; tiny; prove by micro-oracle against CSP-run recordings only where the effect is visible (lock-up, repeated hits). The brake limit is observable in a hard-braking recording. |
| Strut damper sign and FIX_STRUT_FFB | M | needs a strut car (plain Kunos cars with STRUT: 29 front / 5 rear of 113) recorded under CSP; sign question (open question 3). |
| DWB `addForceAtPos` tie-rod factor | S code, M proof | affects FFB / `steerTorque` of every DWB car; needs an FFB / `steerTorque` recording under CSP (shared memory `steer` or `ac.getCar().ffb`). |
| Wet-grip law (`ext+0xb4`), dynamic-grip undo | M | formula is in the hook; needs RainFX conditions + a wet recording. |
| Embree / multi-ray contact (63-119 rays, weights, per-slice load) | L | needs a fast ray structure over the track meshes (rustyAC has the track ray caster, one ray per wheel); results feed the thermal model; prove by comparing contact point / load on kerbs against CSP's Debug lines. |
| CSP V10 force + `SCTM::solve` | L | closed code, 14 KB; unknown differences from `sctm.rs` (bit-exact to AC). First step is the decompile (requests 1-2); then compare numerically with the existing SCTM oracle fed to CSP. |
| Layered thermal model V4 | L | 33-parameter table, per-compound layers, performance map, rim / brake coupling; no AC counterpart. |
| COSMIC | XL | new ODE joint kinds (hinge, slider, rack, compliant wishbone, motors), 120+ keys, setup bindings, wheel body, K&C tests; `docs/port/suspensions.md` has only the four stock types; the double-precision / libccd ODE of CSP is not bit-comparable. |
| Virtual rain / ice compounds, type classification | M | rule-based; needs the generator's output (viewable with CspDebug "View tyres.ini") to port exactly. |
| Extra links / alteration | M | only matters for cars that define them; none installed except possibly the VRC cars. |

What can be reused: `tyre/vanilla_tyre.rs`, `sctm.rs`, `thermal.rs`, `brush.rs`, `car/suspension_{strut,axle,ml}.rs`, `heave_spring.rs`, `antiroll_bar.rs`, `curve.rs`, the oracle tools (`tyre_oracle`, `car_oracle`, `chassis_compare`).

## 8. Open questions and not mapped

1. **Is CSP's V10 force (0x6b4170) + solver (0x69c4f0) equal to AC's for stock data?** Only a decompile or a numerical A/B settles it. Everything from `TYPE`/`SAT_*`/`COMBINED_FACTOR_1`/`FALLOFF_*_CURVE` etc. defaults to "no effect" only by assumption.
2. **Pressure penalty of `getCorrectedD`.** CSP's `Tyre::getCorrectedD` omits the stock `1 / (|P_dyn - P_ideal| * gain + 1)` factor; the term may be re-applied inside 0x6b4170 / 0x69c4f0 / TM, or truly lost. Test: stock car with 10 psi cold vs 26 psi (ideal), same load / slip, A/B.
3. **Strut damper sign.** The marker flips the damper speed for the strut's own damper; with the force convention of `docs/map/suspension.md` 5.3 (`f * dir`, `dir` mount -> foot) the result looks non-dissipative. Either the marker address does not equal the damper the original uses (offset `+0x168` unverified), or the CSP strut step wrapper changes the application elsewhere (the 0x2695085-gated block), or acs.exe's convention differs from the notes. Needs: acs `SuspensionStrut::step` vs CSP behaviour on one strut car, or the decompile of 0x6a3bd0.
4. **Which key is which byte** in 3.2 (names by behaviour only).
5. **Why `ext+0xb4` and the three blow-out thresholds** (phase 6): sources not read.
6. **What `0x6b7f10` (initCompounds closure) and `0x6b6b80` do.**
7. **The 52 key names without text**: reading mechanism (hash / obfuscated table) not analysed.
8. **`getPracticalTemp`**: decompile shows integer arithmetic (lost float return); v2-3 ring sampling inferred.
9. **Gyro / precession torque** (phase 21) is inferred from the shape of the call chain, not from the constants.
10. **How the COSMIC object occupies a `Suspension` allocation** (size 0x210 in acs) and where tag 5 is set.
11. **ODE double-precision core** (other brief) makes bit-exact checks of COSMIC impossible; statistical checks only.
12. The 7 `.pdata` chunks that Ghidra splits (e.g. `0x69c4f0` + `0x69c6fe`) are treated as one function; boundaries unconfirmed.

## Evidence

* `re/scratch/csp1/decomp_hooks/{6ac2a0,6a9fc0,6aa310,6aa5a0,6aab80,6abfc0,6b22e0,6b2640,6b2ce0,6b2dc0,6b40e0,6b68b0,6b68c0,6b6a00,6b6b40,6b8fc0,6b9070,6b93a0,6b94a0,6b98d0,6b9d00,6b9fa0,6ba0f0,6a33d0,6a3400,6a3490,6a3600,6a3630,6a36c0,6a3770,6a37a0,6a3840,6a3a50,1122cc0,1122db0,1122ea0,1122f40,1123000,58be00,5a7210,5eb720,5bb080,5bb250,5bb2d0}.c`
* Bytes of `dwrite_copy.dll` (hand-decoded, read-only): registration stubs `0x2a0510` (Suspension::step), `0x2572c0` (SCTM::solve), `0x25d030`; `0x69d700`, `0x6a3320`, `0x6a3900`, `0x6a3bd0`, `0x858a30`, `0x6ac1c0`; call / slot scans of `0x6a9b60`, `0x6b2e90`, `0x6b4170`, `0x11232b0`.
* Tables: `csp_hook_table.tsv`, `hook_groups.tsv`, `dll_funcs.tsv`, `dll_func_strings.tsv`, `dll_strings.tsv`.
* `docs/map/tyre.md`, `docs/map/suspension.md`, `docs/port/tyre_step.md`, `docs/port/suspensions.md`, `docs/port/grip_investigation.md`, `crates/rustyac-physics/src/tyre/*.rs`, `car/{suspension,antiroll_bar,heave_spring}.rs`, `curve.rs`.
* `prior/csp_survey.md` section 4 (P6, P17-P19, P24), `prior/alpha2026_csp.md` 3.2-3.3, `prior/alpha_keys_csp_tyres.txt`.
* `extension/internal/lua-sdk/ac_car_cphys/lib.lua` (lines 4503-4560, 4870-4920, 9300-9450, 9480-9620, 9880-9960, 10100-10220), `internal/lua-apps/CspDebug/CspDebug.lua` (1205-1360), `config/general.ini` 183-206.
* `Documents/Assetto Corsa/logs/custom_shaders_patch.log` lines 269-337.

## Decompile requests

1. `0x69c4f0` (+ continuation `0x69c6fe`, about 4.2 KB): CSP's `SCTM::solve`; list every term that differs from AC's `SCTM::solve` (`docs/port/sctm.md`) for a stock V10 tyre.
2. `0x6b4170` (10,040 B): CSP `addTyreForcesV10`; is the pressure penalty applied here, which fields feed the layered model, how `TORQUE_MODE_EX` is used.
3. `0x6b2e90` (4,532 B): `addGroundContact` extras (rim contact, tread cover, spring model).
4. `0x6b1950` (2,411 B): batched ray executor and sine / Perlin noise; origin and direction of the single ray.
5. `0x6a9b60` (1,115 B) and `0x6b6b80`: `Tyre::Tyre` extras; where `SMOOTH_LOAD_SENS` is read and what the car-index field `Tyre+0x654` is.
6. `0x6b7f10` (initCompounds closure): what it re-reads / builds.
7. `0x8cc0e0` (30,533 B): CSP's tyres.ini compound loader; which fields of `TyreCompoundDef` it fills (`+0x358` meaning).
8. `0x8d81a0`, `0x8d6a60` (3,183 B), `0x8d4470` (1,266 B), `0x8d42a0`, `0x8d4400`, `0x8d4970`, `0x8d5160`, `0x8d76d0`, `0x8d6020`, `0x8d6090`, `0x8d8d70`: the TM methods used by `Tyre::step` (per-step overrides, vertical stiffness, rim contact, post-force hook).
9. `0x8f83d0`, `0x8f8600`, `0x8fa810`, `0x8fc780`, `0x8f2240`: layered carcass model (input, step, parameters).
10. `0x6b9500`, `0x6b9d80`, `0x6ba160`, `0x6ba300`: thermal-model practical temperature, init, input, post-construct.
11. `0x6ac1c0` (hub proxy) with its vtable `0x2221f98`: what the proxy does with forces; meaning of `Tyre+0x5a4` bit 2.
12. `0x11232b0` (4,808 B): DWB step wrapper (DWB2 / PID / LUT).
13. `0x6a3bd0` (strut step wrapper body, about 0x190 B) and `0x2d3d50`, `0x2d43d0`: hash-keyed strut geometry cache and the TLS marker; settle the damper sign.
14. `0x87f080` (60,194 B), `0x87e090`, `0x87e3e0`, `0x88f380`, `0x88e600`, `0x88eaf0`: COSMIC loader / creation; size of the object, where tag 5 is set, exact key list.
15. `0x897ec0` (15,374 B): per-step COSMIC update.
16. `0x8a9c80` (15,178 B), `0x8b0240` (4,026 B), `0x8b1200`: DWB2 selection, heave-spring update, setup items.
17. `0x82d5c0` (16,681 B), `0x831840`, `0x8328c0`, `0x8342f0`: damper LUT loader / evaluator (hysteresis).
18. `0x84af80`, `0x84b9a0`, `0x6966a0`, `0x6831b0`: extra-link creation, alteration, lazy parse, print.
19. `0x917c30`, `0x919d00`, `0x91cfb0`, `0x91e2d0`, `0x922b60`, `0x923a10`: tyre classification and virtual compound generation (exact rules and generated keys).
20. `0xaf75f0` (358 B): the ambient-pressure function of the tyre pressure law (inputs; the T_ref source `car+0x138 -> +0x100`).
21. `0xed1770` / `0xed17f0`: the per-car object lookup used by `Tyre::step` (is it ever null for a stock car?).
22. `0x7f8ae0` (5,693 B): car-loader piece; where `RIM_SPRING_RATE` / `RIM_DAMPING_RATE` / `TORQUE_MODE_EX` are stored.
23. `0x5bb330`: the Curve helper called from `Tyre::setCompound`.
