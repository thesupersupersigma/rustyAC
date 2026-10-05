# Tyre::step port: the rest of the tyre (Task 04)

Code: `crates/rustyac-physics` (`tyre/vanilla_tyre.rs`, `tyre/thermal.rs`, `tyre/interfaces.rs`,
`tyre/status.rs`, `tyre/data.rs`, `tyre/rig.rs`, `vecmath.rs`, `data/ini.rs`,
`data/tyres_ini.rs`), `tools/tyre_oracle` (new), `tools/disasm.py` (new).
Sources: the disassembly and pseudo-C of `acs.exe` (build timestamp `0x5a55e7a8`) and `acs.pdb`.
Nothing in the game folder or the Ghidra project was changed and no game process was touched.
Only the tyres.ini `VERSION >= 10` path was ported.

---

## 1. Plain-English summary

The whole tyre now exists in Rust, not just the grip formula: how hard it is pressed into the
road, how much it slips, the forces it hands to the suspension, how its wheel speeds up and
slows down, how it heats up and cools, its pressure, wear, graining, blistering, flat spots,
dirt and tyre blankets. To check it, a helper program builds a real Assetto Corsa tyre using the
game's own code (the game itself never starts), bolts it to a pretend wheel hub over a pretend
road, and drives it through twelve scripted situations such as a cold start, braking until the
wheel locks, a wheelspin launch, a long corner, a kerb, a burnout until the tyre bursts, and
pure random input. The Rust tyre is given exactly the same inputs and, after every one of
97,800 steps per car, about 280 numbers are compared digit for digit. For three cars (the F2004,
a made-up test tyre, and an installed car that uses lookup tables) every number of every step
was the same. The one caveat is deliberate nonsense input: when the tyre is fed "not a number",
both sides produce "not a number" in the same places, but the exact internal marking of that
value can differ, so those steps are counted as "the same" rather than "identical bits".
The part that reads a car's tyre file was ported too and checked against what the game itself
loads, for 113 cars: 73,826 loaded values, none different. Two things you asked for turned out
differently: none of the tyre files in the SDK's `v1.5_tyres_ac` folder are the new format
(all 100 are `VERSION=7`), and `f2004ButBetter` has exactly the same data as the stock F2004.

---

## 2. Match results

### Step 1 — data loading coverage

Method (`tyre_oracle coverage`): for each car the game's own `Tyre::init` reads tyres.ini and its
`.lut` files; every value it stored is compared with the Rust loader's (bit patterns, curve
points, compound names, the thermal patches' neighbour lists); then `SCTM::solve` on the game's
object is compared with `VanillaSctm` on 2,000 random inputs per compound (same random mix as
Task 03).

| Set | Cars | Compounds (front + rear) | ... with lookup curves | Loaded values compared | Different | SCTM rows | Bit-exact | % |
|---|---|---|---|---|---|---|---|---|
| `sdk\dev\v1.5_tyres_ac` | 0 of 100 | - | - | - | - | - | - | - |
| `ks_ferrari_f2004` | 1 | 6 | 0 | 736 | 0 | 12,000 | 12,000 | 100 |
| `f2004ButBetter` | 1 | 6 | 0 | 736 | 0 | 12,000 | 12,000 | 100 |
| **Extra, not asked for:** every installed car with a `data.acd` (includes the two above) | 113 | 514 | 24 | 73,826 | 0 | 1,028,000 | 1,028,000 | 100 |
| Synthetic test tyre (`tests/golden/synthetic_car`) | 1 | 4 | 2 | 536 | 0 | 80,000 | 80,000 | 100 |

- **The SDK folder has nothing to test.** All 100 files in `sdk\dev\v1.5_tyres_ac\<car>\data\tyres.ini`
  say `VERSION=7`, so none is on the V10 path. The task assumed otherwise.
- **`f2004ButBetter`**: extracting with the folder's own name gives garbage; with
  `ks_ferrari_f2004` as the key name it gives clean text (`tools/acd_extract.py` got a `--name`
  option for this). Its tyres are `VERSION=10`. All 70 extracted files are byte-identical to the
  stock F2004's, so it adds no new tyre data.
- **The extra 113 cars.** Because the SDK set was empty, I extracted the `data.acd` of every car
  in the game's `content\cars` (read-only, into the git-ignored `cardata\`) and ran the check on
  all of them. All are `VERSION=10`; 51 have the `ks_` prefix, the rest are other installed cars.
  The only ones with `DY_CURVE` / `DX_CURVE` / `DCAMBER_LUT` are `vrc_formula_alpha_2026` (10
  compounds) and `vrc_formula_alpha_2026_csp` (14). Say so if you would rather not have had
  those extracted; deleting `cardata\` undoes it.
- The existing `sctm_compare` was also run on 22 CSVs (F2004, f2004ButBetter,
  vrc_formula_alpha_2026; 20,000 rows each, 440,000 rows): all match. That needed one fix: it
  forced `useSmoothDCamberCurve` off unless `--dcamber-smooth` was given, which was wrong for a
  car that sets `DCAMBER_LUT_SMOOTH=1`.

Loader quirks confirmed against the game with deliberately awkward copies of the synthetic file:
a UTF-8 BOM, CR LF line endings and a Ctrl-Z load the same on both sides; a byte that is not
valid UTF-8 (a Latin-1 `°`) makes the game stop reading the file right there, and the port does
the same (2 of 4 compounds loaded on both sides).

### Step 3 — Tyre::step, every step of every scenario

Car: F2004 (`ks_ferrari_f2004`). "Bit-exact" = all ~280 recorded values of the step have AC's
bits. "Same" additionally treats any NaN as equal to any NaN (see below).

| Scenario | Axle | Steps | Bit-exact | % | Same (NaN = NaN) | % | First divergence |
|---|---|---|---|---|---|---|---|
| warmup | front | 6,000 | 6,000 | 100 | 6,000 | 100 | none |
| warmup | rear | 6,000 | 6,000 | 100 | 6,000 | 100 | none |
| brake_lockup | front | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| brake_lockup | rear | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| wheelspin | front | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| wheelspin | rear | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| cornering | front | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| cornering | rear | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| liftoff | front | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| liftoff | rear | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| blankets | front | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| blankets | rear | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| bump | front | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| bump | rear | 4,000 | 4,000 | 100 | 4,000 | 100 | none |
| overheat | front | 6,000 | 6,000 | 100 | 6,000 | 100 | none |
| overheat | rear | 6,000 | 6,000 | 100 | 6,000 | 100 | none |
| random (fixed seed) | front | 8,000 | 8,000 | 100 | 8,000 | 100 | none |
| random (fixed seed) | rear | 8,000 | 8,000 | 100 | 8,000 | 100 | none |
| tester (no car) | front | 2,000 | 2,000 | 100 | 2,000 | 100 | none |
| tester (no car) | rear | 2,000 | 2,000 | 100 | 2,000 | 100 | none |
| nan_inputs | front | 2,400 | 100 | 4.17 | 2,400 | 100 | none |
| nan_inputs | rear | 2,400 | 100 | 4.17 | 2,400 | 100 | none |
| golden_mix | front | 500 | 500 | 100 | 500 | 100 | none |
| golden_mix | rear | 500 | 500 | 100 | 500 | 100 | none |
| **Total** | | **97,800** | **93,200** | **95.30** | **97,800** | **100** | |

The same 24 recordings were made for the synthetic test tyre and for `vrc_formula_alpha_2026`
(lookup curves, smooth camber table, `COMBINED_FACTOR`): identical totals, no divergence.

**The `nan_inputs` rows.** That scenario injects a NaN or an infinity into one input every 200
steps. From the first injection (step 100) on, some state stays NaN for good (the wheel phase,
for one). Every value that is NaN in AC is NaN in the port and every other value has AC's bits,
but the NaN's sign bit differs in places (`0xffc00000` against `0x7fc00000`). Which NaN an
addition returns depends on the order the compiler gives its operands; Rust does not promise
one, so this cannot be pinned down from source code. I did not chase it further.

Checks that the comparison is honest:

- Changing one constant by its last bit (`0.33333334` to `0.3333333` in the thermal model) drops
  the cornering recording to 66 of 4,000 steps, first divergence at step 0 in
  `thermal.practicalTemp`.
- The scenarios do what their names say (`tyre_compare --stats`, F2004 front): load 0 to
  31,717 N, locked for 1,423 steps and a flat spot of 0.56 in brake_lockup, slip ratio up to 9.7
  in wheelspin, core temperature 22 to 746 C and the tyre deflated for 5,704 steps in overheat,
  blister up to 7.6, wear up to 46 virtual km in random, the rim reached in bump, 149 steps
  without ground and 2,379 body-drag calls in random. All three torque modes, driven and
  non-driven wheels and three compounds are used.
- The first two versions of the scripts were too tame or too wild (the front wheelspin torque
  first never broke traction, then burst the tyre), and blistering and punctures were not reached
  at all until the overheat scenario was added. Everything was re-recorded after each change.

What is recorded per step: every hub call in order (`addForceAtPos`, `addTorque`,
`addLocalForceAndTorque`, and the body's `addForceAtLocalPos`) with all its arguments, all 40
members of `TyreStatus`, 31 other `Tyre` members (contact point and normal, road axes, sliding
and road velocities, `localMX`, ...), `localWheelRotation`, and the thermal model (phase, core,
grip multiplier, 36 patch temperatures and pending heat).

A checked-in test (`tests/tyre_golden.rs`, runs in `cargo test`) replays 500 steps of
`golden_mix` (front) and the first 400 of `liftoff` (rear, the curve compound) on the synthetic
tyre and compares a hash of the full state after every step. It uses made-up tyre numbers, so no
car data is in the repository.

---

## 3. The interfaces

### `Suspension` (AC's `ISuspension`)

Slot names are from the PDB (the `Suspension` class's vtable). The tyre calls six of 25:

| Slot | Offset | Name | What it means for the tyre |
|---|---|---|---|
| 1 | +0x08 | `getHubWorldMatrix()` | Hub transform. Row 0 = axle (x), row 1 = up (y), row 2 = z (the rolling direction is this axis), row 3 = wheel centre. |
| 2 | +0x10 | `getPointVelocity(p)` | World velocity of the hub at world point `p` (asked at the contact point). |
| 3 | +0x18 | `addForceAtPos(force, pos, driven, addToSteerTorque)` | World force at a world point. Vertical load: `addToSteerTorque = false`. Grip force (original torque mode): `true`. `driven` is the tyre's own flag. |
| 4 | +0x20 | `addTorque(torque)` | World torque: the aligning torque about the ground normal; in `reactionTorques` mode also brake + handbrake + electric torque about the axle. |
| 7 | +0x38 | `getHubAngularVelocity()` | World angular velocity of the hub; its part about the axle is added to the wheel speed. |
| 24 | +0xc0 | `addLocalForceAndTorque(force, torque, driveTorque)` | Replaces slot 3 for the grip force when the torque mode is not `original`. Despite the name all three are world vectors. |

Other slots, for reference: 0 destructor, 5 `setSteerLengthOffset`, 6 `getSteerTorque`,
8 `attach`, 9 `getStatus`, 10 `getBasePosition`, 11 `getK`, 12 `getDamper`, 13 `getPackerRange`,
14 `getDebugLines`, 15 `setDamage`, 16 `resetDamage`, 17 `getDamage`, 18 `getMass`, 19 `stop`,
20 `getVelocity`, 21 `getSteerBasis`, 22 `step`, 23 `setERPCFM`.

### `RayTrackCollisionProvider` (AC's `IRayTrackCollisionProvider`)

| Slot | Offset | Name | Meaning |
|---|---|---|---|
| 1 | +0x08 | `rayCast(org, dir, result, length)` | One ray from 2 m above the wheel centre, straight down, length 2. Result: hit or not, point, normal, `SurfaceDef`. |
| 2 | +0x10 | `rayCastWithRayCaster` | Not called by the tyre. |
| 3 | +0x18 | `createRayCaster(length)` | `Tyre::init` asks for a per-wheel `IRayCaster` (length 3). If it gets one, `step` casts through that (`IRayCaster::rayCast`, slot 1; `release` is slot 2) and takes the surface from the hit object. |

The Rust trait has one method, `ray_cast`, returning `Option<RayCastResult>`; both of the game's
paths give the tyre the same three things. From `SurfaceDef` the tyre reads `gripMod`,
`dirtAdditiveK`, `sinHeight`, `sinLength`, `damping` and `granularity`.

### `TyreCar`

Not asked for, but needed: `Tyre::step` reads its `Car*` in many places. The trait holds
exactly those: `torqueModeEx`, `isSleeping()`, `getSpeed()`, and through `car->ksPhysics`
`track->dynamicGripLevel`, `tyreConsumptionRate`, `mechanicalDamageRate`, `ambientTemperature`,
`roadTemperature`, `allowTyreBlankets`; and three calls on `car->body` used only on surfaces with
`damping > 0`: `getVelocity` (+0x78), `getMass` (+0x28), `addForceAtLocalPos` (+0xf8). It is
passed as `Option`, because the game's tyre test bench runs with a null car and the code branches
on that.

`TyreModel` (AC's `ITyreModel`) gained two methods the game does not have as virtuals:
`get_static_dy` (the game calls `SCTM::getStaticDY` on its own object directly) and
`set_compound`. `VanillaTyre` owns a `Box<dyn TyreModel>`.

---

## 4. Tools

- **rea**: did not connect at all this session ("connection timed out after 30000ms"). Not used.
- **kawaiidra**, bridge tools: `list_analyzed_binaries` and `get_function_decompile` worked for
  all 13 calls. The first call still stalled (120 s, until the next request). I used the
  `limit: 1` + read-from-cache trick from last time to keep the pseudo-C out of the conversation.
  `batch_decompile` and `get_xrefs_to` were not retried.
- **Most of the reading did not go through an MCP.** Exact float order needs the disassembly, and
  pulling ~7,000 instructions through MCP calls was not practical, so I wrote `tools/disasm.py`:
  capstone (installed with pip into the git-ignored `tools/_pydeps`) plus the PDB through the DIA
  SDK. It finds functions by name, disassembles with call targets, imports, constants and string
  literals named, resolves vtables, and lists parameter names. The vtable slot names, struct
  layouts and every float expression in this port come from it.

Decompiler traps this time: none of the kind Task 03 hit in the float maths that mattered, but
the port was written from the disassembly, so I did not rely on that. Things the pseudo-C hid or
got wrong: the single characters `INIReader::parse` searches for; the order of the status
initialisation in `Tyre::Tyre` (it clears the struct *after* the member initialisers, so
`inflation` and `wearMult` start at 0, not 1); and NaN behaviour of comparisons throughout.

---

## 5. Unknown, unverified, not ported

**Not ported.**

1. The `VERSION < 10` path (`Tyre::addTyreForces`, `stepRelaxationLength`, the brush model), as
   instructed. `VanillaTyre::init` refuses such a file. The loader reads the old keys but not the
   brush provider's derived numbers.
2. `onStepCompleted` (a callback) and `shakeGenerator`; `Tyre::step` does not use either for its
   own results.
3. `evOnTyreCompoundChanged` and `PhysicsEngine::isTyreLegal` (a race rule that drops compounds).
4. Reading from an encrypted `data.acd` (`INIReader::loadEncrypt`, `Curve::loadEncrypted`). The
   loader refuses a file that sits next to a `<folder>.acd` rather than behave differently.

**Not verified.**

1. **The game normally loads tyres from `data.acd`, the oracle from extracted files.** Those are
   different code paths in `INIReader` and `Curve` (text decoding in particular). Values come
   out the same for plain ASCII files as far as I can reason, but it was not tested.
2. **The oracle's shortcuts.** `INIReader::useCache` is switched off in the private mapping (the
   cache is a static map that only the game's start-up code constructs). The tyre is created
   like the game's test bench does (`Tyre::init` with a null car), then the fake car is attached
   and `setCompound` is called; in the game `init` gets the car directly. The fake hub returns
   the same velocity for every point. The fake car answers constants.
3. **`XMMatrixInverse`** was written from the DirectXMath source as I remember it, not from the
   disassembly. It is exercised by every step in the two non-original torque modes and matches,
   which is evidence, not a reading of the code.
4. **Number parsing without the 2013 runtime.** With `MSVCR120.dll` the game's own `wcstod` is
   called. The fallback parser has not been compared with it.
5. **Driven wheels** are emulated by overwriting the wheel speed before each step. What the
   drivetrain really does with `feedbackTorque` is still unread.
6. Surfaces: the rig's road is a plane straight under the wheel with one surface; nothing about
   the real track collision was looked at.

**Answers to Task 01's open questions.** 1 (slot names): done, section 3. 2 (`TyreModelData::Fz0`):
confirmed in the game's memory, it stays 2000 for every one of 514 compounds. 5 (`buildTyre`,
`getIMO`, `getPatchAt`): ported; the neighbour lists match the game's for all 36 patches.

**Loader behaviour worth knowing** (all the game's, all reproduced):

- A line with both `[` and `]` anywhere is a section header, even inside a comment.
- A repeated key keeps its first value; a repeated section header empties the section.
- Keys are not trimmed; values only at the end. Numbers are read up to the first character that
  does not belong to a number.
- A missing `CX_MULT` gives 0, not 1. A missing thermal section leaves the grain and blister
  gains at 0. A `.lut` line with a `|` whose parts are not numbers crashes the game; the port
  returns an error. Two test tables from Task 03 had such a comment line and were changed.

---

## 6. Commands

All from the repository root.

```
cargo build --release
cargo test
cargo build --release --manifest-path tools/tyre_oracle/Cargo.toml
```

Extract a car (key name only needed for a renamed folder):

```
python tools/acd_extract.py --car f2004ButBetter --name ks_ferrari_f2004 --out cardata/f2004ButBetter
```

Step 1, loader and SCTM coverage (one `--car` per car; prints the table of section 2):

```
tools\tyre_oracle\target\release\tyre_oracle.exe coverage --car cardata/ks_ferrari_f2004 --car cardata/f2004ButBetter --n 2000
tools\tyre_oracle\target\release\tyre_oracle.exe coverage --car cardata/ks_ferrari_f2004 --n 20000 --csv-out oracle/sctm_cov
target\release\sctm_compare.exe --car cardata/ks_ferrari_f2004 --axle front --compound FRONT_1 --csv oracle/sctm_cov/ks_ferrari_f2004_front_FRONT_1.csv
```

Step 3, record AC's `Tyre::step` and compare (`--check` compares straight away; the second
command is the standalone comparison and prints the per-scenario table):

```
tools\tyre_oracle\target\release\tyre_oracle.exe run --car cardata/ks_ferrari_f2004 --check
target\release\tyre_compare.exe --car cardata/ks_ferrari_f2004 --stats oracle/tyre/ks_ferrari_f2004_front_warmup.csv
```

`run` options: `--axle front|rear|both`, `--scenario <name>|all`, `--out <dir>` (default
`oracle/tyre`, git-ignored, about 9 MB per recording), `--steps <n>`, `--golden`. Both commands
take `--acs <path>` and `--verbose` (shows the game's own console output). Exit code 0 means
everything matched.

Regenerate the checked-in golden files:

```
set G=crates/rustyac-physics/tests/golden
tools\tyre_oracle\target\release\tyre_oracle.exe run --car %G%/synthetic_car --axle front --scenario golden_mix --golden --out %G%
tools\tyre_oracle\target\release\tyre_oracle.exe run --car %G%/synthetic_car --axle rear --scenario liftoff --steps 400 --golden --out %G%
```

Read the game's code:

```
python -m pip install --target tools/_pydeps capstone
python tools/disasm.py find "Tyre::step*"
python tools/disasm.py dis 140283800
```
