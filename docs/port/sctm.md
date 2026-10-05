# SCTM port: the first Vanilla physics piece (Task 03)

Code: `crates/rustyac-physics` (new), `tools/sctm_oracle` (extended), `tools/make_sctm_golden.py`
(new). Sources: the disassembly and pseudo-C of `acs.exe` (build timestamp `0x5a55e7a8`) read
through the kawaiidra MCP, `acs.pdb` for names, and the oracle from Task 02. Nothing in the game
folder or the Ghidra project was changed and no game process was touched.

---

## 1. Plain-English summary

The formula Assetto Corsa uses to turn "how hard is the tyre pressed down and how much is it
slipping" into grip forces now exists in Rust. It is a line-by-line copy of the game's own code,
including the small lookup-table helper it can use. To check it, a helper program asked the real
game code and the Rust copy the same questions and compared the answers digit for digit: the
9,734 tidy questions from the last task, 200,000 new random ones covering odd situations (a
wheel barely touching the ground, a locked wheel, a car almost standing still, worn and
blistered tyres, wrong pressure), and 120,000 more that switch on parts of the formula the
F2004 does not normally use. Every single answer was identical down to the last binary digit,
for front and rear tyres alike: 329,734 questions, seven numbers each, zero differences. The
maths functions (sine, tangent, power and so on) are taken from the very same system file the
game uses, so this is not luck. A small set of 208 of these questions is stored in the project
and re-checked automatically by `cargo test`. Still missing: reading lookup tables from a car's
data files (the F2004 has none), and everything around the formula, which is the next task.

---

## 2. Match results

Car: F2004 (`ks_ferrari_f2004`), compound Slick Soft. "Bit-exact" means all seven output fields
(`Fy, Fx, Mz, trail, ndSlip, Dy, Dx`) of a row have the same 32 bits as AC's output. Max error
is over all seven fields, in absolute units and in ULPs (steps between neighbouring floats).

| Sweep | Axle | Rows | Bit-exact | Max abs error | Max ULP error |
|---|---|---|---|---|---|
| lateral | front | 968 | 100 % | 0 | 0 |
| longitudinal | front | 488 | 100 % | 0 | 0 |
| camber | front | 2,904 | 100 % | 0 | 0 |
| combined | front | 507 | 100 % | 0 | 0 |
| lateral | rear | 968 | 100 % | 0 | 0 |
| longitudinal | rear | 488 | 100 % | 0 | 0 |
| camber | rear | 2,904 | 100 % | 0 | 0 |
| combined | rear | 507 | 100 % | 0 | 0 |
| random (seed 1) | front | 100,000 | 100 % | 0 | 0 |
| random (seed 1) | rear | 100,000 | 100 % | 0 | 0 |
| random, `combinedFactor` 1.5 (seed 2) | front | 20,000 | 100 % | 0 | 0 |
| random, `combinedFactor` 1.5 (seed 2) | rear | 20,000 | 100 % | 0 | 0 |
| random, 3 lookup curves, linear camber table (seed 3) | front | 20,000 | 100 % | 0 | 0 |
| random, 3 lookup curves, linear camber table (seed 3) | rear | 20,000 | 100 % | 0 | 0 |
| random, 3 lookup curves, smooth camber table, `combinedFactor` 3 (seed 4) | front | 20,000 | 100 % | 0 | 0 |
| random, 3 lookup curves, smooth camber table, `combinedFactor` 3 (seed 4) | rear | 20,000 | 100 % | 0 | 0 |
| **Total** | | **329,734** | **100 %** | **0** | **0** |

The first ten rows are the target of the task (8 sweeps + 100,000 random rows per axle). The
last six are extra: the F2004 never reaches those branches (`COMBINED_FACTOR`, `DY_CURVE`,
`DX_CURVE`, `DCAMBER_LUT`), so the oracle forces them on with made-up tables
(`crates/rustyac-physics/tests/golden/synthetic_*.lut`), filled into the game's object with the
game's own `Curve::addValue`.

Checks that the comparison itself is honest:

- Regenerating the 8 grid sweeps with the extended oracle gives byte-identical files.
- Feeding the comparer a wrong parameter (`--combined-factor 1.4` against the 1.5 data) drops it
  to 1,887 of 20,000 rows matching, so it does detect differences.
- The random rows are not all one case. Front, 100,000 rows: 5,137 hit the all-zero early exit,
  81,213 are past the grip peak and 13,650 before it, 43,217 are braking, 993 are at the
  locked-wheel slip-ratio limit, 10,046 use the simple model. No row produced NaN or infinity.

### What the random sweep covers

`sctm_oracle --sweep random --n <count> --seed <seed>` draws each input from a mix of a wide
normal range and the special values the code branches on:

| Input | Mix |
|---|---|
| `load` | 90 % 50…12000 N, 5 % 0.001…50, 3 % exactly 0, 2 % negative |
| `slipAngleRAD` | 70 % ±0.35, 20 % ±1.5, 5 % exactly 0, 5 % ±0.0001 |
| `slipRatio` | 60 % ±0.4, 20 % −1.2…3, 10 % exactly 0, 5 % −1…−0.999, 5 % ±0.0001 |
| `camberRAD` | 70 % ±0.1, 15 % exactly 0, 15 % ±0.5 |
| `speed` | 80 % 0…100 m/s, 10 % 0…1, 5 % exactly 0, 5 % −5…0 |
| `u` | 85 % 0.3…1.6, 15 % exactly 1 |
| `cpLength` | 0…0.3 m |
| `grain` | 50 % 0, 50 % 0…100 |
| `blister` | 50 % 0, 40 % 0…100, 10 % −20…150 |
| `pressureRatio` | 30 % 0, 70 % −0.8…0.6 |
| `useSimpleModel` | 10 % on |

Not covered: NaN or infinite inputs (the port is written to branch like the original on NaN,
but that is untested), and any car other than the F2004.

---

## 3. Maths functions

`acs.exe` does not use the UCRT: it imports its maths from **`MSVCR120.dll`** (Visual C++ 2013).
The import thunks `SCTM::solve` and its helpers call were resolved from the exe's import table:

| Call site target | Import | Used for |
|---|---|---|
| `0x14039997a` | `MSVCR120!sinf` | camber, slide speed |
| `0x140399974` | `MSVCR120!cosf` | slide speed |
| `0x140399a0c` | `MSVCR120!tanf` | slip angle; also `maxSlip0/1` in `initCompounds` (`float tan(float)` at `0x1400bf630` is a jump to it) |
| `0x14039996e` | `MSVCR120!powf` | load sensitivity, `calcLoadSensMult`, `combinedFactor` |
| `0x14044ff96` | `MSVCR120!sqrtf` | slide speed, combined slip |

All arithmetic in these functions is single f32 SSE instructions; there is no f64 maths (the two
`fabs` calls go float → double → float, which changes nothing).

What `crates/rustyac-physics/src/math.rs` does:

- `sinf`, `cosf`, `tanf`, `powf`: **swapped to the game's own DLL.** On Windows `msvcr120.dll`
  is loaded at run time and called directly. If it is missing (or on another OS) Rust std is
  used and `math::backend()` says so. `RUSTYAC_MATH=std` forces the fallback.
- `sqrtf`: Rust's `f32::sqrt`. A square root has only one correctly rounded answer; a unit test
  checks 2 million random inputs against `MSVCR120!sqrtf`.

Was the swap needed? Measured on this machine: **no difference was found.** Rust std (which ends
up in the UCRT) and MSVCR120 gave identical bits for 20 million random inputs per function
(`cargo test --release -- --ignored --nocapture std_vs_msvcr120`), and running the whole
comparison with `RUSTYAC_MATH=std` is also 100 % bit-exact. The swap is kept anyway because it
makes "same result as AC" true by construction instead of by a measurement on one PC with one
Rust version. If the dependency on the 2013 runtime ever becomes a problem, dropping it costs
nothing that has been observed so far.

One thing not investigated: whether `MSVCR120.dll` itself returns different bits on different
CPUs (it picks code paths by CPU feature). The oracle and the port run the same DLL on the same
CPU, so they would agree either way.

No FMA and no reassociation: Rust never contracts or reorders float operations on its own, the
port has no `mul_add`, and the crate sets no fast-math style flags.

---

## 4. How it was ported, and what the decompiler got wrong

Every function was transcribed from its **disassembly**. The pseudo-C in `re/tyre/decomp/` is
right about what is computed but regroups three products, which would each cost a last-bit
error:

| Where | Pseudo-C | Real code |
|---|---|---|
| `solve`, slide speed | `(cos · speed) · slipRatio` | `cos · (speed · slipRatio)` |
| `solve`, trail | `((3 − 2t) · t) · t` | `(3 − 2t) · (t · t)` |
| `getPureFY`, before the peak | `((1−n)² · k) · slip + ((3 − 2n) · n) · n` | `(1−n)² · (k · slip) + (3 − 2n) · (n · n)` |

Ported functions (each has its name and address in a doc comment):

| AC function | Address | Rust |
|---|---|---|
| `SCTM::SCTM` | `0x14044b8c0` | `VanillaSctm::default` |
| `SCTM::solve` | `0x14044bc20` | `<VanillaSctm as TyreModel>::solve` |
| `SCTM::getStaticDY` / `getStaticDX` | `0x14044bbb0` / `0x14044bb30` | `get_static_dy` / `get_static_dx` |
| `SCTM::getPureFY` | `0x14044ba70` | `get_pure_fy` |
| `calcLoadSensMult` | `0x14027f770` | `tyre::calc_load_sens_mult` |
| `Curve::getCount` / `addValue` | `0x1402068e0` / `0x140205ae0` | `Curve::get_count` / `add_value` |
| `Curve::getValue` | `0x140206990` | `Curve::get_value` |
| `Curve::getCubicSplineValue` | `0x1402068f0` | `Curve::get_cubic_spline_value` |
| `CubicSpline<float,float>::computeCoefficients` | `0x140205c20` | `curve::compute_coefficients` |
| `CubicSpline<float,float>::valueAt` | `0x140207410` | `curve::value_at` |

Layout: `tyre/mod.rs` has the slot (`trait TyreModel`, AC's `ITyreModel`) and
`TyreModelInput` / `TyreModelOutput`; `tyre/sctm.rs` has `VanillaSctm`; `curve.rs` the lookup
table; `data/tyres_ini.rs` the tyres.ini → parameters step; `math.rs` the runtime functions;
`tyre/oracle_csv.rs` the CSV reader shared by the comparer and the test.

Naming: struct fields are the PDB member names in snake_case (`lsMultY` → `ls_mult_y`).
Function parameters use the PDB names where the PDB has them (`getPureFY(D, cf, load, slip)`,
`calcLoadSensMult(targetD, targetLoad, sensExp)`, the spline's `h, l, u, z, a, b, c, d`).
`SCTM::solve` has no local names left in the PDB (only `this` and `in`), so its locals are
descriptive names of my own.

Two deliberate differences from the original, neither visible in the results:

- `solve` writes `1.0` into `this->asy` for `useSimpleModel` and restores it at the end. The
  port passes the value along instead, so `solve` takes `&self` and one model can be shared
  between threads.
- The spline is built on first use through a `OnceLock` instead of a `cubicSplineReady` flag.

---

## 5. Not understood, not verified, not done

**Constants.** Working through the algebra explains more than Task 01 could:

- `getPureFY` is the classic brush tyre model. With `n = slip / peakSlip`, the code before the
  peak expands to `3n − 3n² + n³`. `0.0064` is probably `0.08²` (half contact-patch length
  squared, as in the brush model's `2·c·a²` stiffness); that reading is mine, not confirmed.
- `78.125` is exactly `1 / (2 · 0.0064)`, so the `3 · 78.125` in `solve` cancels the
  `2 · 0.0064 / 3` in `getPureFY`: the grip peak lands exactly on `maxSlip` (then moved by
  grain and pressure). The two constants only exist to express stiffness in brush-model units.
- Still plain unexplained tuning numbers: `0.75` (how much the peak slip follows the grip
  multiplier `u`), `0.2` (blister: up to 20 % less grip), the trail shape `0.8`, `1.1`, `0.1`,
  `0.12` (trail is roughly 12 % of the contact patch length at zero slip and goes slightly
  negative past the peak), and the `−0.9` that replaces a camber loss of −100 % or worse.
- `dCamberBlend` is only ever the constructor's `1.0`. The SCTM members `dy0`, `dx0`, `pacE`,
  `pacCf`, `pacFlex` are never written or read by the game and are left out of the port.
- `getPureFY` receives `D` and `load` and ignores both.

**Not verified.**

1. **tyres.ini → parameters.** `data/tyres_ini.rs` repeats the transforms Task 02 read from the
   pseudo-C of `Tyre::initCompounds` / `setCompound`. Its output equals what the oracle puts
   into the game's object (checked by a test), but the oracle hand-mirrors the same reading, so
   both would share a mistake. In particular the grouping of
   `tan((FLEX_GAIN + 1) · FRICTION_LIMIT_ANGLE · 0.017453)` was taken from the pseudo-C, not
   from the disassembly. Letting the game run `Tyre::init` itself (Task 02, open question 3) is
   still the real check.
2. **Number parsing.** ini values are parsed as double and narrowed to float.
   `INIReader::getFloat` was not decompiled.
3. **NaN / infinite inputs**, see section 2.
4. **Only the F2004** was compared (three compounds exist; Slick Soft was used, the others
   differ in `DY_REF`/`DX_REF` only).

**Not done.**

1. **Reading lookup curves from car data.** `Curve::load` and `INIReader::getCurve` are not
   ported, so `data/tyres_ini.rs` refuses a compound that has `DY_CURVE`, `DX_CURVE` or
   `DCAMBER_LUT` (same as the oracle). The model itself handles curves and is tested with them;
   `Curve::from_lut_text` is a plain `x|y` reader used for the synthetic test tables only.
2. Nothing of `Tyre::step` or any other system, as instructed.

Also worth knowing: `docs/map/tyre_oracle.md` describes the oracle before this task; the new
options are listed in section 6 below. The golden test stores the F2004's derived SCTM parameter
values and 208 result rows in the repository (numbers computed from Kunos car data).

---

## 6. Commands

All from the repository root (`C:\Users\thesupersupersigma\Desktop\project5\rustyAC`).

Build and test:

```
cargo build --release
cargo test
cargo test --release -- --ignored --nocapture std_vs_msvcr120    (optional: MSVCR120 vs Rust std)
```

Run the comparison on one file (exit code 0 = everything bit-exact, 1 = differences):

```
target\release\sctm_compare.exe --car cardata/ks_ferrari_f2004 --axle front --csv oracle/sctm/ks_ferrari_f2004_front_random.csv
```

Full CLI: `sctm_compare --car <dir> --axle front|rear --csv <file> [--compound <name>]
[--combined-factor <x>] [--dy-curve <lut>] [--dx-curve <lut>] [--dcamber-lut <lut>]
[--dcamber-smooth]`. The optional overrides must be the ones the CSV was generated with. It
prints the SCTM parameters, then per output field: rows, bit-exact count and %, max abs error,
max ULP error, and the worst row with its inputs.

Regenerate the oracle data (needs the game installed and the Visual C++ 2013 x64 runtime;
repeat every line with `--axle rear`):

```
cargo build --release --manifest-path tools/sctm_oracle/Cargo.toml
set O=tools\sctm_oracle\target\release\sctm_oracle.exe
set G=crates/rustyac-physics/tests/golden

%O% --car cardata/ks_ferrari_f2004 --axle front --sweep all
%O% --car cardata/ks_ferrari_f2004 --axle front --sweep random --n 100000 --seed 1
%O% --car cardata/ks_ferrari_f2004 --axle front --sweep random --n 20000 --seed 2 --combined-factor 1.5 --tag cf15
%O% --car cardata/ks_ferrari_f2004 --axle front --sweep random --n 20000 --seed 3 --dy-curve %G%/synthetic_dy_curve.lut --dx-curve %G%/synthetic_dx_curve.lut --dcamber-lut %G%/synthetic_dcamber.lut --tag curves
%O% --car cardata/ks_ferrari_f2004 --axle front --sweep random --n 20000 --seed 4 --dy-curve %G%/synthetic_dy_curve.lut --dx-curve %G%/synthetic_dx_curve.lut --dcamber-lut %G%/synthetic_dcamber.lut --dcamber-smooth --combined-factor 3 --tag curves_smooth
```

Output: `oracle/sctm/<car>_<axle>_<sweep>[_<tag>].csv` (gitignored). The matching comparer
overrides for the tagged files are `--combined-factor 1.5`; the three `--…-curve/lut` options;
and the three plus `--dcamber-smooth --combined-factor 3`.

New oracle options: `--sweep random` with `--n` (default 100000) and `--seed` (default 1);
`--combined-factor`, `--dy-curve`, `--dx-curve`, `--dcamber-lut`, `--dcamber-smooth` to force
parameters the car does not set; `--tag` to add a suffix to the file name. Same seed, same file.

Rebuild the checked-in golden file from the oracle data (208 rows, evenly spaced picks):

```
python tools/make_sctm_golden.py
```
