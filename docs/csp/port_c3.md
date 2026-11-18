# Task C3: `--csp`, CSP's extended car data accepted, proven against CSP's own car build (v1.1.0)

The first CSP task that ships code (plan.md row **CSP-1**). CSP target 0.3.2-preview2, build 4305
([version.md](version.md)); every `dwrite+0x…` address below is build 4305's.

## Resume here

> Updated after each commit.

| Step | State |
|---|---|
| 1. the switch (`--csp`, `[CSP] ENABLED`, `CspFeatures`) | done, 7d1db0a |
| 2. CSP's reader rules (VERSION, WAV_PITCH, sections, COSMIC / encrypted reasons) | done, 7d1db0a; oracle `--surfaces` 7808e65 |
| 3. proof area `csp` | done, b6a540b (pressure rule) + 14b4d84; 45 of 45 IDENTICAL |
| 4. quick run, README line, release v1.1.0 | done: quick run 220 cases, 219 IDENTICAL (the one DIFFERENT, `refused off`, was the install changing under the run: fixed in the area, rerun 45/45); tag v1.1.0 |

## How CSP mode is defined

rustyAC's CSP mode is **CSP 0.3.2-preview2 as it runs with a physics worker count of 0** (every
dispatch in list order on one thread, what real CSP does on a 2-thread PC: with more islands on
more threads CSP's physics depends on ODE's global reorder seed and on wall time, so even the
real game can't repeat it) **and with `[PHYSICS_EXPERIMENTS_DONE] RANDOMIZE_WHEEL_ANGLES=0`** (CSP's
default 1 gives each wheel a random start angle from an unseeded per-thread RNG,
`dwrite+0x69e2f0`, the `Tyre::initCompounds` detour; with 0 the car's memory is repeatable). Neither matters
for loading a car beyond that; both are written here because every later CSP task builds on them.

## The switch

- `--csp` on `rustyac.exe`, or `[CSP] ENABLED=1` in `cfg\rustyac.ini` (docs/flags.md). Off by
  default. It is a process-wide switch (`rustyac_physics::csp::set_enabled`), set before any
  content loads, and written into recordings (`SimSetup.csp`, header key `csp`, only when on, so
  every recording made before reads back unchanged: plan.md decision 2).
- **Off = Vanilla.** Every CSP rule below is behind `csp::enabled()`; with it off the code paths are
  the old ones, which every existing proof area runs.
- `CspFeatures` per car (`VanillaCar.csp`, from `CspFeatures::for_car` when the game builds the car)
  and per track (`SurfacesManager.csp` → `Track.csp`): `extended: Option<(v2, N)>`, the
  `WAV_PITCH=extended-N` sections with their values, and every section whose name starts with `_`
  (`[_EXTENSION]`, `[_SCRIPTING_PHYSICS]`, …) of every `.ini` in the car's data (archive or folder)
  or the track's `surfaces.ini`, kept with all their keys. What CSP does with them is later tasks
  (plan.md decision 1: the behaviour behind each seam becomes options of our own modules; nothing
  is hooked).
- `acd_check --csp` and `track_survey --csp [--wav-pitch]` read content as CSP mode does.

## What CSP's reader accepts, and how rustyAC now reads it

### `car.ini` `[HEADER] VERSION`

acs.exe reads it in `CarAvatar::initCommon` (acs+0xd56e0), not in the physics `Car::Car`: when the
file is read and has the key, `(short)getInt` (`std::stoi`, which throws on `extended-2`: the car
does not load). rustyAC did not read it at all, so the BDC and RSS cars loaded without CSP, unlike
the game. **Now, with `--csp` off, rustyAC reads it as acs.exe does** (crates/rustyac-game `build_car`
and `acd_check`): `VERSION=extended-2: invalid stoi argument (this car is made for Custom Shaders
Patch: run with --csp)`. The physics crate and its proofs (`car_oracle`, `chassis_compare`) build
`Car::Car`, which never reads it, so they are unaffected.

CSP's `getInt` hook (`dwrite+0x5e3590`) changes only this one read (armed by its wrapper of
`CarAvatar::initCommon`, `dwrite+0x110e630`, disarmed after the first match):

| `VERSION=` | CSP returns |
|---|---|
| `extended-<s>` (case-sensitive) | `parse_int(s, 0)` truncated to 32 bits |
| `extendedV2-<s>` | the same, from after `extendedV2-` |
| empty, or a first character that is not a digit (`Extended-2`, `foo`, `-1`) | 2 |
| starts with a digit | the original `getInt` (`std::stoi`: `2a` is 2, out of range throws) |

`parse_int` (`dwrite+0x15d58e0`): `0x`/`0X` → hex digits until the first non-hex one (0 with none);
otherwise an optional `-`, then decimal digits with 64-bit wrap; no digits → the default (so
`extended-+2` is 0). `csp::car_version`, unit tests in `csp.rs`.

### The wing rule (`hasSection` hook `dwrite+0x5dfd20`)

Armed only by CSP's wrapper of `NetCarStateProvider::initWings` (`dwrite+0x61e6f0`): a
`DYNAMIC_CONTROLLER_n` section whose `WING=N` has no `[WING_N]` reads as absent. rustyAC has not
ported `initWings` (online wing angles), and `AeroMap::loadINI`'s own `DYNAMIC_CONTROLLER` loop is
not under the rule, so **nothing in rustyAC changes**: the rule lands with `initWings`.

### `surfaces.ini` `WAV_PITCH=extended-N`

CSP's `getFloat` hook (`dwrite+0x5e4180`): for a file whose name ends `/data/surfaces.ini`, a section
starting `SURFACE_` and the key `WAV_PITCH`, a value starting `extended-` is read with
`parse_float(rest, 0.0)` (`dwrite+0x15d5b90`: sign, decimal digits in double, `.` fraction by
repeated ×0.1, `e`/`E` exponent through `pow10` `dwrite+0x15d58a0`) and the track gets CSP's track
extension; anything else goes to the original `getFloat` (which refuses `extended-0` without CSP).
`csp::wav_pitch`; the oracle's call-through (`csp_oracle --surfaces`, through the game's own
`INIReader::getFloat` with CSP live) gave the odd-spelling vectors in the unit test
(`extended-1.5`, `extended-+2e1`, `extended-`, `extended--0.25E-1`, `extended-.`, `extended-7x`,
`extended-0.1e400` = inf, …), all equal.

The hook is gated on CSP's state object `dll+0x2821078` holding, at `+0x3b8`, an object whose `+0x60`
is zero; the oracle sets a zeroed stand-in there (`shortcut:` comment in csp_oracle; it is taken to
be the `TrackAvatar` being built, whose `trackObjects` vector is empty while its `SurfacesManager`
loads). Open question below.

### CSP sections, COSMIC, encrypted content

- Sections whose name starts with `_` are parsed and kept in `CspFeatures.sections` (file, name,
  keys in order); unknown ones are kept, never errors (Vanilla ignores them too).
- COSMIC suspension under `--csp`: the physics message plus `(COSMIC suspension: a later rustyAC
  version)`. Never a crash (tests/replay.rs `an_unsupported_car_is_refused_with_the_physics_message`
  checks both modes).
- `urd_darche_992_23` (`extendedV2-2`): its data files are encrypted by its maker for CSP; rustyAC
  never decrypts them (rule 5) and refuses it: `(the file has no readable sections: this car's data
  is encrypted for Custom Shaders Patch by its maker, and rustyAC does not decrypt it)`.

### The tyre pressures after a reset (`Tyre::reset` detour `dwrite+0x69e8d0`)

CSP's `Tyre` constructor detour (`dwrite+0x69de90`) reads `tyres.ini [COMPOUND_DEFAULT] INDEX` as an
int16 (`dwrite+0x15fbb20`, the same parse as `parse_int`), its `Tyre::initCompounds` detour keeps
that compound's static pressure (compound 0's for an index that does not exist), and its
`Tyre::reset` detour sets both pressures to it before every reset, including the reset inside
`setCompound`. So under CSP a car is built at the default compound's pressure, where acs.exe
builds it at compound 0's (they meet again at session start, when the setup screen picks the
default compound; a later reset under CSP goes back to the default compound's pressure, whatever
compound is on). rustyAC's CSP mode does the same (`VanillaTyre::csp_reset_pressure`). Found by the
proof: the `bmw_m3_e30` (31 psi compound 0, `INDEX=2` at 26 psi) was the only car of the 18 where
the two differ. For cars with a CSP physics extension the detour runs its own reset instead of the
game's; that one is not ported (`shortcut` #55 in docs/port/shortcuts.md: at build it leaves the same
memory, the lithium's `words` case).

### The first lap (`TimeTransponder::init` detour `dwrite+0x69a070`)

CSP arms `isFirstLapArmed` (Car+0x278) at build when race.ini's `[SESSION_0] SPAWN_SET=HOTLAP_START`
and `TYPE <= 2` (read in `dwrite+0x1402bd0`). rustyAC's CSP mode does the same
(`sim.rs` `first_lap_armed`, unit test); Vanilla arms it only for hotlap sessions (type 4).

## Content: before and after

`acd_check --steps 1 --refused` without and with `--csp`, `track_survey` without and with `--csp`
(the area's `refused off|csp` and `layouts` cases).

| | without `--csp` | with `--csp` |
|---|---|---|
| 10 `bdc_streetspec_{350z,ae86,altezza,aristo,e36,miata,r32,rx7,s13,s14}_v4` | refused (VERSION) — before this task they loaded | **load** |
| `rss_formula_2010`, `rss_formula_hybrid_2026`, `rss_formula_rss_3_v6_2026` (`extended-2`, DWB; content_needs.md missed them) | refused (VERSION) — before they loaded | **load** |
| `rss_formula_rss_3_v6` (`extended-1`, DWB: Content Manager rewrote its `data.acd` in the install at 22:01 during this task, leaving `data.acd~cm_bak_ep`; before that it was a plain car) | refused (VERSION) | **load** |
| `vrc_formula_lithium_2023_csp` (DWB, `[_EXTENSION]` kept) | refused (VERSION) | **load** |
| `vrc_formula_alpha_2025_csp`, `alpha_2026_csp`, `beta_2024_csp` | refused (VERSION) | refused: COSMIC suspension, a later rustyAC version |
| `urd_darche_992_23` | refused (VERSION) | refused: encrypted by its maker |
| `ks_barcelona/layout_moto_fa_2026`, `ks_red_bull_ring/layout_gp_fa_2026`, `ks_silverstone/gp_fa_2026`, `montreal/montreal_fa_2026`, `monza/monza_fa_2026`, `spa/spa_fa_2026` | refused (`WAV_PITCH=extended-0`) | **load** (SURFACE_0's pitch 0.0, as CSP reads it) |

15 cars and 6 layouts newly accepted (plan.md's CSP-1 row predicted 14; the 15th became a CSP car in the install during the task). Every other car and layout
is unchanged.

## The car's memory, against CSP's own build

The game's `Car` is 0x3ea0 bytes = 2,004 words. rustyAC has no exporter of a whole `Car` in the
game's layout (it would map about 1,645 non-pointer words), so the proof is a chain of two links,
each word by word or value by value against the game:

1. **CSP's build against the game's Vanilla build** (`words <car>`, `tools/proofs/csp_words.py`):
   `csp_oracle` builds the car with CSP live, and again without CSP from a copy of its data with
   `VERSION=extended-N` read as `N` (the only way Vanilla builds it). All 2,004 words are compared,
   pointers left out (they are addresses). Every word must be equal except CSP's own:

   | word | what CSP writes | in rustyAC |
   |---|---|---|
   | Car+0x278 | `isFirstLapArmed` = 1 (hot-lap start, TYPE ≤ 2) | modelled: the case requires 0 / 1 |
   | each Tyre +0x344, +0x348 | `pressureStatic` / `pressureDynamic` = the default compound's | modelled: the case requires `acd_check --csp --pressures`'s values |
   | Car+0x2d8 | `FileChangeObserver.lastChanged` = -1 (CSP's own file watcher) | not modelled: no file watching |
   | Car+0x3630/0x3638 | `screenName` length (CSP's name is `ui_car.json`'s) | not modelled: the physics car has no name |
   | Car+0x3db4, +0x3e2d, +0xaec | CSP's flag / zeroed padding | padding: no field |
   | each Tyre +0x40c, +0x4e1, +0x64e, +0x829 | the default compound's pressure / its index, tyre index, `SMOOTH_LOAD_SENS` in padding | padding: no field (the pressure is `csp_reset_pressure`) |

   `RANDOMIZE_WHEEL_ANGLES=0` (CSP mode's definition) keeps the wheel matrices out of it. No word
   is zeroed on the oracle's game side for this task: rule 18's list in differences.md is unchanged
   (C2's `Car+0x2b0` stack zeroing and the fixed `RtlGenRandom` stream are csp_oracle's own,
   documented in inprocess.md 3.4).
2. **The game's Vanilla `Car::Car` against rustyAC** (`car <car>`): `car_oracle run --car <car>
   --scenario wc_stops` and `chassis_compare run --dir`, the existing car proof (every named field of
   every step, bit for bit). `Car::Car` never reads `VERSION`, so this is the same car as CSP mode
   builds; the CSP words of link 1 are either modelled (the first lap) or not physics.

Result (area `csp`, 45 of 45 IDENTICAL):

| car | words compared (pointers left out) | CSP's own words | `words` | `car` (wc_stops, 4,334 values) |
|---|---|---|---|---|
| 10 `bdc_streetspec_*_v4` | 1,652 or 1,649 (352 or 355) | 20 | IDENTICAL | IDENTICAL |
| `rss_formula_2010` | 1,428 (576) | 18 | IDENTICAL | IDENTICAL |
| `rss_formula_hybrid_2026` | 1,482 (522) | 19 | IDENTICAL | IDENTICAL |
| `rss_formula_rss_3_v6` | 1,428 (576) | 19 | IDENTICAL | IDENTICAL |
| `rss_formula_rss_3_v6_2026` | 1,500 (504) | 18 | IDENTICAL | IDENTICAL |
| `vrc_formula_lithium_2023_csp` | 1,372 (632) | 15 | IDENTICAL | IDENTICAL |
| `ks_ferrari_f2004` (plain) | 1,645 (359) | 17 | IDENTICAL | IDENTICAL |
| `bmw_m3_e30` (plain) | 1,658 (346) | 26 | IDENTICAL | IDENTICAL |
| `bmw_z4_gt3` (plain) | 1,644 (360) | 18 | IDENTICAL | IDENTICAL |

With `--csp` off the plain cars are the existing Vanilla car proofs (`physics` and `fresh`, unchanged
and IDENTICAL in the quick run) and the extended cars are refused (`refused off`, 19 cars).

## Proof rows: area `csp`

A new area (`tools/proofs/csp.sh`, lane with `fresh`, all of it in the quick set:
`tools/proofs/quick/csp.txt`), not rows in `physics`: it needs `csp_oracle` and the CSP junction
`re/csp/fakeac`, which `physics` (saved recordings only, never the game) must not. Cases:
`words <car>` and `car <car>` for the 15 accepted cars and 3 plain ones (`ks_ferrari_f2004`,
`bmw_m3_e30`, `bmw_z4_gt3`), `refused off`, `refused csp`, `layouts`, `wav <track> <layout>` ×6.
It runs in about 6.5 min. The cars need their data in `cardata/` (ignored; `tools/acd_extract.py`).

## Repeatability

Nothing new for differences.md: CSP mode's wheel angle rule is part of its definition (above), not a
difference from the game it runs as.

## Not done

- The wing rule in rustyAC: lands with `NetCarStateProvider::initWings`.
- What CSP does with `[_EXTENSION]` / `[_SCRIPTING_PHYSICS]` keys (later CSP tasks): kept only.
- CSP's `SCREEN_NAME` / `SHORT_NAME` from `ui_car.json` (`dwrite+0x5e1b50`) for the HUD/menus.
- rustyAC's `desktop.rs` reads `[INFO] SHORT_NAME` for every car; acs.exe only when `VERSION > 1`
  (old, small mismatch found on the way).

## Open questions

- The `WAV_PITCH` hook's gate (`dll+0x2821078` → `+0x3b8` → `+0x60 == 0`): its writer was not found;
  the oracle's stand-in assumes the `TrackAvatar` being built.
- CSP's `expectedFuelPerLap` scale (`Car+0x3d38` ×= `max(sim+200, 0.1)`, `dwrite+0x587080`): null in
  the oracle, so no difference; what `+200` is in the real game is unread.
- `vrc_formula_lithium_2023_csp`'s data is unpacked in `cardata/` (ignored) from its standard
  `data.acd`, as for every other mod car; its physics scripts stay scrambled and unread.
- `tools/proofs/areas_test.sh` fails one line at HEAD before this task too:
  `tools/car_oracle/Cargo.lock -> [physics fresh sound replays], want []`.
- `chassis_compare`'s clippy fails on `input_file.rs:133` (`to_bytes(&self)` on a `Copy` type), old
  code included by path; untouched.
