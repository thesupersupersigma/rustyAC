# Tyre ground truth: TyreTester, .acd extraction and the SCTM oracle (Task 02)

Sources: `acs.exe` + `acs.pdb` (Ghidra project `ghidra/acs/acs.gpr`, read through the kawaiidra
MCP), the F2004's `data.acd` (read only). Nothing in the game folder, the Ghidra project or git
was changed, and no running game process was touched. No Rust port of the tyre model was written.

---

## 1. Plain-English summary

The oracle is a small program that borrows the game's own tyre formula. It opens `acs.exe` as a
plain block of code inside its own process (the game itself never starts), fills in the numbers
for one F2004 tyre from the car's data file, and then asks the game's formula "how much grip do
you give for this load, this amount of sideways slip and this amount of wheel spin?" several
thousand times. Every question and every answer is written to a spreadsheet-style file, so the
future Rust version of the formula can be checked against the real thing number by number.

What the F2004 curves show (soft slick, 20 m/s, ideal pressure and temperature, clean track):

- Sideways grip rises almost in a straight line, peaks when the tyre is pointed about 6° away
  from its direction of travel at the front (about 5.5° at the rear), and then slowly fades: at
  15° the front still gives about 94 % of its peak.
- Forward grip peaks at about 11–12 % wheel spin, and braking grip at about 9–10 % wheel lock.
  Braking grip is 4 % stronger than acceleration grip.
- The harder the tyre is pressed down, the more total grip it gives, but less per unit of load:
  a front tyre gives about 2.3 times its load at 1000 N, 1.75 times at 4000 N and 1.52 times at
  8000 N. The rear tyre loses grip with load faster (2.73 → 1.80 → 1.46).
- The peak moves to a slightly larger slip angle as load rises (6.0° → 6.25° front).
- Leaning the wheel helps on one side and hurts on the other: at 4000 N, 2° of lean gives about
  3 % more sideways grip in one direction and about 6 % less in the other.
- Asking for forward and sideways grip together shares one budget: at 4° of slip and 10 % wheel
  spin the front tyre gives 4000 N sideways instead of 6640 N.

---

## 2. Part A — Kunos' TyreTester

### How it is opened

- `TyreTesterForm` (a `ksgui::Form`, internal name `TYRE_TESTER`, title "Tyre tester") is
  created exactly once, in `Sim::Sim` @ `0x140192070` (call at `0x140193e8c`), for the player's
  car (`cars[0]`), together with the other developer forms (`CarPhysicsForm`, `TelemetryApp`,
  `FormWings`, `ACSuspension`, …). It is always constructed; there is no hotkey and no Python
  hook for it.
- Each of those forms is handed to `Sim::addForm(form, devAppsEnabled)` @ `0x140197ae0`.
  `ksgui::Form::Form` sets `devApp = true` by default, and `addForm` does this:
  - dev apps **disabled** and `form->devApp`: the form is registered with flag `0` and hidden
    (`setVisible(false)`);
  - otherwise: registered with flag `1` (the normal path every in-game app takes).
- `devAppsEnabled` is read in `Sim::Sim` from **`system/cfg/assetto_corsa.ini`**:
  `[AC_APPS] ENABLE_DEV_APPS`, true when the value is `> 0`. If the file cannot be read or has no
  `[AC_APPS]` section the flag stays false.
- The tester does nothing while a replay is playing (`ReplayManager::isInReplaymode`).

### What it sweeps and shows

`TyreTesterForm::render` @ `0x14033bc30` re-runs everything every frame for the wheel chosen with
its spinner (0–3 = FL, FR, RL, RR), using that wheel's **current compound** and its **current
live load** (`car->physicsState.load[i]`). Speed is the car's speed, or 15 (converted through
`Speed`) when the car is below 30.

| Sweep | Function | Range | Drawn as |
|---|---|---|---|
| Slip angle | `TyreTester::slipAngleSweep` @ `0x14044f300` | −20°…+20°, 200 steps, slip ratio 0 | series 0 = Fy / load; series 2 = Mz scaled by load |
| Slip ratio | `TyreTester::slipRatioSweep` @ `0x14044f710` | −1…+1, 200 steps | series 1 = Fx / load |
| Combined | `TyreTester::srSASweep` @ `0x14044fb00` | slip ratio −1…+1 at a fixed 9° slip angle | series 4 = Fy / load, plus a friction-ellipse line (Fx vs Fy) drawn with a `GLRenderer` |
| Load sensitivity | `Tyre::getDY` | 0…2000 kg in 10 kg steps | series 3 |
| Camber | `Tyre::getCamberedDy` | −20°…+20° step 0.2° | series 5 = (value − 1) × 5 |

Each sweep drives a private, car-less `Tyre` through the full `Tyre::step(0.003)` with
`externalInputs` overriding load / slip angle / slip ratio, and records `status.Fy, Fx, Mz,
slipAngle, slipRatio` (`TyreTestFrame`, 20 bytes). Under the 500×500 graph a label shows
`FY:<peak Fy/load> @ <slip angle>   FX:<peak> @ <slip>   MZ:<peak> @ <angle>   DY @ 0.5k / 1k / 2k`.

**Results are on screen only.** No file is written by the form or by the three sweep functions
(`tyre->debugOutput` is set to true by the sweeps, but no code that reads it was found).

Caveat for V10 cars: the load-sensitivity and camber lines call `Tyre::getDY` /
`getCamberedDy`, which are the **pre-V10** formulas. For the F2004 those two lines would be
drawn from the legacy `DY0/DY1` keys, not from what `SCTM::solve` uses. The Fy/Fx/Mz sweeps do go
through the V10 path.

### Manual steps (not done by me — no config file was edited)

1. Open `C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\system\cfg\assetto_corsa.ini`.
2. In section `[AC_APPS]` change `ENABLE_DEV_APPS=0` (line 15 right now) to `ENABLE_DEV_APPS=1`.
   Keep a note of it: a Steam "verify files" will put it back to 0.
3. Start any offline session (practice is easiest) and get in the car.
4. Move the mouse to the right edge of the screen to open the app bar and pick **Tyre tester**
   from the list (it sits with the other dev apps such as the car-physics and suspension forms).
5. Use its spinner to pick the wheel (0–3). The curves follow the wheel's live load, so they
   move while driving.

Step 4 is inferred, not observed: the code shows the form being registered the same way as the
normal apps once the flag is on, but I did not run the game. If it is not in the app bar, that
registration flag is the thing to look at (`gameScreen` vtable slot `+0xa8`).

---

## 3. Part B — `.acd` extraction

### AC's own reader

- `INIReader::loadEncrypt` @ `0x140237570` → `FolderEncrypter::decryptFile` @ `0x14023bdd0`
  (walks the container, returns one file's bytes).
- `ksSecurity::keyFromString` @ `0x1402cfe00` builds the key. The decompiler dropped its
  arithmetic, so it was transcribed from the disassembly.
- (`SimpleFileEncrypter::decryptFile` @ `0x14023b650` is the same byte scheme for single files
  with a caller-supplied key; not used for car data.)

### Format

```
[int32 -1111, int32 keyId]          optional header (new-style files)
repeat until end of file:
    int32  nameLength
    bytes  name (UTF-8)
    int32  size                      number of plain bytes
    int32  data[size]                one int per plain byte; only the low byte is used
plain[i] = (data[i] - key[i % len(key)]) & 0xff
```

The key is text: eight numbers `"%d-%d-%d-%d-%d-%d-%d-%d"`, each the low byte of a small integer
hash of the **car folder name** (`Path::getFileName(Path::getPath(<…/data.acd>))`), using the low
byte of every character as a signed char `c[i]`, `n` characters, 32-bit wrapping arithmetic and
x86 truncating division:

| # | start | loop | step |
|---|---|---|---|
| 1 | 0 | `i = 0; i < n; i++` | `+= c[i]` |
| 2 | 0 | `i = 0; i < n-1; i += 2` | `r = r*c[i] - c[i+1]` |
| 3 | 0 | `i = 1; i < n-3; i += 3` | `r = r*c[i]; r = r / (c[i+1] + 27); r += -27 - c[i-1]` |
| 4 | 0x1683 | `i = 1; i < n; i++` | `-= c[i]` |
| 5 | 0x42 | `i = 1; i < n-4; i += 4` | `r = (c[i]+15)*r; r = (c[i-1]+15)*r + 22` |
| 6 | 0x65 | `i = 0; i < n-2; i += 2` | `-= c[i]` |
| 7 | 0xab | `i = 0; i < n-2; i += 2` | `r = r % c[i]` |
| 8 | 0xab | `i = 0; i < n-1; i++` | `r = r / c[i] + c[i+1]` |

For `ks_ferrari_f2004` the key is `179-44-163-59-166-193-14-53`.

In the game, when the header is present `keyId` must be in a global `keys` list or the read
returns nothing. `tools/acd_extract.py` skips the 8 header bytes and does not check the id.

### Tool and result

`tools/acd_extract.py --car <car folder> --out <dir>` opens only `<car folder>/data.acd`,
read-only, and refuses to write inside the car folder. Run for the F2004 it wrote 70 files
(`.ini`, `.lut`, `.rto`) to `cardata/ks_ferrari_f2004/`; the output is clean text, which is the
check that the key and byte scheme are right.

**`tyres.ini`: `[HEADER] VERSION=10`** → the SCTM path, so Part C applies.

| Index | Sections | NAME | SHORT_NAME |
|---|---|---|---|
| 0 | `FRONT` / `REAR` | Slick Soft | S |
| 1 | `FRONT_1` / `REAR_1` | Slick Medium | M |
| 2 | `FRONT_2` / `REAR_2` | Slick Hard | H |

`[COMPOUND_DEFAULT] INDEX=1` (Medium). `[VIRTUALKM] USE_LOAD=1`. The three compounds differ only
in `DY_REF`/`DX_REF` (1.81 / 1.80 / 1.79), wear and thermal curves, and one rear inertia value.
No compound uses `DY_CURVE`, `DX_CURVE`, `DCAMBER_LUT` or `COMBINED_FACTOR`.

---

## 4. Part C — the SCTM oracle (`tools/sctm_oracle/`)

Calling AC's code directly **worked**; the time cap was not needed. `SCTM::solve` needs no game
global state: only its own object, its two argument structs and the C runtime's maths functions.

### How it calls AC's code

1. `LoadLibraryExW(<original acs.exe path>, NULL, DONT_RESOLVE_DLL_REFERENCES)` maps the exe as
   an image with relocations applied. No entry point, no static constructors, no imports.
2. Two checks before anything is called: the PE timestamp must be `0x5a55e7a8`, and slot 1 of
   `SCTM`'s vftable must point at `SCTM::solve`. A different game build stops with an error.
3. Imports of `MSVCR120.dll` and `MSVCP120.dll` only are bound by hand in the private mapping
   (`sinf`, `cosf`, `tanf`, `powf`, `sqrtf`, `operator new`). No other game DLL (Steam, FMOD,
   Direct3D, …) is loaded. This is the only memory written; the file on disk is untouched.
4. Functions used, by RVA (Ghidra address − `0x140000000`, names from `acs.pdb`):

   | RVA | Symbol | Use |
   |---|---|---|
   | `0x44b8c0` | `SCTM::SCTM` | builds the object, including its three empty `Curve`s |
   | `0x44bc20` | `SCTM::solve` | the sweep itself |
   | `0x1416580` | `SCTM::vftable` | sanity checks |
   | `0x27f770` | `calcLoadSensMult` | `lsMultX/Y`, same call `initCompounds` makes |
   | `0xbf630` | `float tan(float)` | `maxSlip0/1`, same call `initCompounds` makes |
   | `0x14196dc` | float `0.017453` | the game's own degrees→radians literal |

### Struct setup

`SCTM` is 0x1e8 bytes of zeroed, 8-byte-aligned memory passed to the game's constructor, then
filled at the offsets in `re/tyre/types/SCTM.txt`, mirroring `Tyre::initCompounds` →
`Tyre::setCompound` for `VERSION >= 10`. `TyreModelInput` (0x30) and `TyreModelOutput` (0x1c)
are `#[repr(C)]` structs with compile-time size checks.

| SCTM field | From tyres.ini | Front soft | Rear soft |
|---|---|---|---|
| `lsExpY`, `lsExpX` | `LS_EXPY`, `LS_EXPX` | 0.8, 0.8 | 0.7, 0.71 |
| `lsMultY` | `calcLoadSensMult(DY_REF, FZ0, LS_EXPY)` | 9.231399 | 21.7838 |
| `lsMultX` | `calcLoadSensMult(DX_REF, FZ0, LS_EXPX)` | 9.231399 | 20.05019 |
| `Fz0` | `FZ0` | 3451 | 3995 |
| `maxSlip0` | `tan(FRICTION_LIMIT_ANGLE · 0.017453)` | 0.110399164 | 0.101574585 |
| `maxSlip1` | `tan((FLEX_GAIN + 1) · FRICTION_LIMIT_ANGLE · 0.017453)` | 0.11362784 | 0.10451066 |
| `asy` | `FALLOFF_LEVEL` | 0.9 | 0.9 |
| `falloffSpeed` | `FALLOFF_SPEED` | 7 | 7 |
| `speedSensitivity` | `SPEED_SENSITIVITY` | 0.003228 | 0.002914 |
| `camberGain` | `CAMBER_GAIN` | 0.213 | 0.26 |
| `dcamber0`, `dcamber1` | `DCAMBER_0`, `DCAMBER_1` (0.1 / −0.8 if either is 0) | 1.4, −13 | 1.4, −13 |
| `cfXmult` | `CX_MULT` | 1 | 1 |
| `pressureCfGain` | `PRESSURE_FLEX_GAIN` | 0.3 | 0.3 |
| `brakeDXMod` | `1 + BRAKE_DX_MOD` (1 if absent or 0) | 1.04 | 1.04 |
| `combinedFactor` | `COMBINED_FACTOR`, else 0 | 0 | 0 |
| `dCamberBlend` | constructor value, never set by `setCompound` | 1 | 1 |
| three `Curve`s | empty | — | — |

### Inputs and sweeps

Fixed inputs (in every CSV row): `speed = 20` (TyreTester's default), `u = 1`, `grain = 0`,
`blister = 0`, `pressureRatio = 0`, `useSimpleModel = 0`, `tyreIndex = 0` (front) or `2` (rear),
`cpLength = ksCalcContactPatchLength(RADIUS, load / RATE)`. `--speed`, `--u` and
`--pressure-ratio` override the first three.

| Sweep | Grid | Rows |
|---|---|---|
| `lateral` | load 1000…8000 step 1000 × slip angle −15°…15° step 0.25°, slip ratio 0, camber 0 | 968 |
| `longitudinal` | same loads × slip ratio −0.30…0.30 step 0.01, slip angle 0 | 488 |
| `camber` | the lateral sweep at camber 0°, −2°, −4° | 2904 |
| `combined` | loads 2000 / 4000 / 6000 × slip angle −12°…12° step 2° × slip ratio −0.30…0.30 step 0.05 | 507 |

CSV columns: `compound, axle, load, slip_angle_deg, slip_angle_rad, slip_ratio, camber_deg,
camber_rad, speed, u, tyre_index, cp_length, grain, blister, pressure_ratio, use_simple_model,
Fy, Fx, Mz, trail, ndSlip, Dy, Dx`. Floats are printed with the shortest text that reads back to
the same 32-bit value, so a port can compare bit for bit; `slip_angle_rad` / `camber_rad` are the
exact values passed to the game. Two runs produce byte-identical files.

The row with slip angle, slip ratio and camber all exactly 0 is all zeros (including `Dy`/`Dx`):
that is `SCTM::solve`'s own early exit, not a gap in the data.

### Assumptions

1. **ini → field mapping is hand-mirrored.** The oracle does not run `Tyre::initCompounds`; it
   repeats the transforms read from its pseudo-C and disassembly. Sanity checks that pass:
   `Dy(Fz0) = DY_REF`, braking/drive peak ratio ≈ 1.04 = `brakeDXMod`, peak near
   `FRICTION_LIMIT_ANGLE`.
2. **Number parsing.** ini values are parsed as doubles and narrowed to float; `INIReader::getFloat`
   was not decompiled to confirm it does the same. A difference would be at most one last-digit
   step in a parameter.
3. **Operating point.** `u = 1` and `pressureRatio = 0` mean "ideal temperature, ideal pressure,
   no wear, full track grip". In the game a cold F2004 tyre has `PRESSURE_STATIC = 10` against
   `PRESSURE_IDEAL = 23`, i.e. `pressureRatio ≈ −0.565`, and `u` below 1.
4. **`cpLength`** ignores the tyre damper and the pressure term of the spring rate; it only
   affects `trail` and `Mz`.
5. **`speed = 20`** feeds the speed-sensitivity term (grip ÷ (1 + sensitivity × slide speed)), so
   peaks are slightly lower at higher speed.
6. Lookup-curve compounds (`DY_CURVE`, `DX_CURVE`, `DCAMBER_LUT`) are refused with an error;
   the F2004 has none.
7. `--compound` picks the compound, but the default file name has no compound in it, so a second
   compound overwrites the first unless `--out` is changed. The checked-in numbers are Slick Soft.

### Key numbers (Slick Soft, 20 m/s, u = 1, pressureRatio = 0, camber 0)

Front:

| Load N | Peak Fy N | Fy / load | Slip angle at peak | Peak Fx N (drive) | Slip ratio at peak | Peak Fx N (brake) | Slip ratio at peak |
|---|---|---|---|---|---|---|---|
| 1000 | 2303.2 | 2.303 | 6.00° | 2301.1 | 0.11 | −2396.2 | −0.09 |
| 2000 | 4010.1 | 2.005 | 6.00° | 4006.2 | 0.12 | −4171.2 | −0.09 |
| 3000 | 5546.3 | 1.849 | 6.00° | 5541.2 | 0.12 | −5768.2 | −0.09 |
| 4000 | 6981.2 | 1.745 | 6.00° | 6975.0 | 0.12 | −7262.6 | −0.10 |
| 5000 | 8345.0 | 1.669 | 6.00° | 8338.0 | 0.12 | −8683.2 | −0.10 |
| 6000 | 9654.6 | 1.609 | 6.25° | 9646.8 | 0.12 | −10046.8 | −0.10 |
| 7000 | 10921.5 | 1.560 | 6.25° | 10912.2 | 0.12 | −11365.3 | −0.10 |
| 8000 | 12152.3 | 1.519 | 6.25° | 12141.3 | 0.12 | −12646.3 | −0.10 |

Rear:

| Load N | Peak Fy N | Fy / load | Slip angle at peak | Peak Fx N (drive) | Slip ratio at peak | Peak Fx N (brake) | Slip ratio at peak |
|---|---|---|---|---|---|---|---|
| 1000 | 2727.1 | 2.727 | 5.50° | 2687.5 | 0.11 | −2798.2 | −0.09 |
| 2000 | 4430.1 | 2.215 | 5.50° | 4396.2 | 0.11 | −4577.3 | −0.09 |
| 3000 | 5883.8 | 1.961 | 5.50° | 5862.7 | 0.11 | −6104.3 | −0.09 |
| 4000 | 7196.1 | 1.799 | 5.50° | 7191.2 | 0.11 | −7487.5 | −0.09 |
| 5000 | 8412.1 | 1.682 | 5.50° | 8425.6 | 0.11 | −8772.7 | −0.09 |
| 6000 | 9556.7 | 1.593 | 5.75° | 9589.7 | 0.11 | −9984.8 | −0.09 |
| 7000 | 10645.4 | 1.521 | 5.75° | 10698.5 | 0.11 | −11139.2 | −0.09 |
| 8000 | 11688.2 | 1.461 | 5.75° | 11761.9 | 0.11 | −12246.3 | −0.09 |

Peaks are the best grid point (0.25° / 0.01 resolution), not an interpolated maximum.

Other points, front at 4000 N: Fy at 15° = 6551.8 N (93.8 % of peak). Camber −2° at ±6° slip
angle: −7212.2 N / +6546.5 N (camber 0: ∓6981.2 N); camber −4°: −7186.9 N / +5973.8 N.
Combined, slip angle 4°: Fy 6644.7 N at slip ratio 0, and Fy 3996.8 N with Fx 5715.7 N at slip
ratio 0.10. Front Slick Hard at 4000 N: peak Fy 6904.1 N.

---

## 5. Rebuild and rerun

From the repository root (`C:\Users\thesupersupersigma\Desktop\project5\rustyAC`):

```
python tools/acd_extract.py --car "C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\content\cars\ks_ferrari_f2004" --out cardata/ks_ferrari_f2004

cargo build --release --manifest-path tools/sctm_oracle/Cargo.toml

tools\sctm_oracle\target\release\sctm_oracle.exe --car cardata/ks_ferrari_f2004 --axle front --sweep all
tools\sctm_oracle\target\release\sctm_oracle.exe --car cardata/ks_ferrari_f2004 --axle rear --sweep all
```

Full CLI:

```
sctm_oracle --car <dir> [--compound <NAME or SHORT_NAME>] [--axle front|rear]
            [--sweep lateral|longitudinal|camber|combined|all] [--out <dir>]
            [--acs <path to acs.exe>] [--speed <m/s>] [--u <mult>] [--pressure-ratio <x>]
```

Defaults: first compound (Slick Soft), `front`, `all`, `oracle/sctm`, the Steam `acs.exe` path.
Output: `oracle/sctm/<car>_<axle>_<sweep>.csv`; the key-number table is printed to the console.
The crate has no dependencies. It needs the 64-bit Visual C++ 2013 runtime the game already uses.
`cardata/` and `oracle/` are in `.gitignore`, so the CSVs are not tracked.

---

## 6. Open questions

1. **TyreTester in the app bar** — step 4 of the manual steps is inferred from `Sim::addForm`,
   not seen in the game. What `gameScreen` vtable `+0xa8` does with its last argument was not
   decompiled.
2. **`INIReader::getFloat` parsing** (assumption 2) should be confirmed before treating the last
   bit of every parameter as exact.
3. **Is the ini mapping complete?** The stronger check is to let the game build the object
   itself: `TyreTester` shows that `Tyre::Tyre` + `Tyre::init` run with `car = NULL`, so calling
   them in the oracle and comparing `Tyre::scTM` with the hand-filled object looks possible. It
   needs `INIReader` to find the car data relative to the working directory and
   `PhysicsEngine::isTyreLegal` to behave without a physics engine; neither was tried.
4. **The `.acd` `keys` list** (accepted key ids for the `-1111` header) was not read; the
   extractor ignores the id.
5. **Operating point for Task 03 tests.** The CSVs cover `u = 1`, `pressureRatio = 0`, no grain
   or blister, `useSimpleModel = 0`. Those branches of `solve` (pressure stiffness, grain,
   blister, the `asy = 1` simple model, non-default `COMBINED_FACTOR`, the three curve lookups)
   are not exercised yet; extra sweeps are a small addition when the port needs them.
6. **Legacy lines in the tester** — the tester's load-sensitivity and camber curves use the
   pre-V10 `getDY` / `getCamberedDy` even on V10 cars (section 2). Whether Kunos intended that
   was not investigated.
7. **kawaiidra tooling this run** — the bridge tools worked (`find_functions`,
   `get_function_decompile`, `get_function_disassembly`, `get_function_xrefs`, `search_strings`).
   `batch_decompile` and `get_xrefs_to` still failed with "No JSON result in output", so
   `tools/re_cache.py` was used to search the cached `Sim::Sim` pseudo-C, and a throwaway script
   read `acs.exe`'s import table and constants. Calls still stall until another MCP request is sent.
