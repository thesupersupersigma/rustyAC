# The owner's installed content against CSP

What in `C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\content` (read-only scan) needs
Custom Shaders Patch 0.3.0-preview649, which CSP feature it needs, and which features would unlock
the most. Method: every car folder was opened the way `acs.exe` opens it (`data.acd` read with the
standard key from the folder name, exactly what `tools/acd_extract.py` and `acd_check` do; nothing
was written out), every `extension/ext_config.ini`, every `surfaces.ini`, every script file name and
every CSP config in `extension/config/{cars,tracks}/{generic,loaded}` was listed. Nothing that CSP or
a mod maker encrypted was opened or decoded. Raw data: `re/scratch/csp1/content_scan.json`,
`content_features.json` (scratch, not committed).

## 1. Inventory

| | Count | Note |
|---|---|---|
| Car folders | 204 | **134 loadable** (have `data.acd`); **70 are DLC placeholders** (only `ui/`, not owned) |
| Track folders | 26 | many layouts; DLC folders without models (`ks_brands_hatch`) |
| Cars with `extension/ext_config.ini` | 20 | 10 `bdc_streetspec_*`, `rss_formula_hybrid_2020`, `urd_darche_992_23`, 8 `vrc_*` |
| Cars with a CSP config downloaded by Content Manager | 8 | `bmw_m3_e30`, `bmw_z4_gt3`, `ks_ferrari_f2004`, `lotus_49`, `lotus_98t`, `lotus_exige_240`, `lotus_exos_125_s1`, `tatuusfa1` |
| Cars with Lua scripts | 5 | `urd_darche_992_23` (3 scripts, encrypted data), `vrc_formula_alpha_2025_csp` (23 files in `data/`), `vrc_formula_alpha_2026_csp` (28), `vrc_formula_beta_2024_csp` (19), `vrc_formula_lithium_2023_csp` (1: `script.lua`) |
| Tracks / layouts with `ext_config.ini` | 3 | `ct_backwoods`, `vhe_interlagos`, `monza_fa_2026` (an inactive include) |
| Track configs downloaded by CM | 11 | `bahrain_international_circuit`, `imola`, `ks_nordschleife`, `ks_nurburgring`, `ks_red_bull_ring`, `ks_zandvoort`, `magione`, `monaco_2017`, `montreal`, `monza`, `spa` (+ 23 helper `.kn5` light / glass models) |
| `generic.ini` | applies to **all 134 cars and every track** | shader fixes, rain-tyre normals, LOD reset, ice spikes, dirt colours, distant reverb on Kunos tracks |

## 2. Cannot be supported

| Item | Why | What was done |
|---|---|---|
| `urd_darche_992_23` | **Its data is encrypted for CSP.** Every file in its `data.acd` decodes to one comment line of scramble (30 files unreadable); `car.ini [_EXTENSION] REQUIRED_VERSION` and `VERSION=extendedV2-2` are the only readable markers; it also carries three Lua scripts (`a_urd_brake_bias.lua`, `a_urd_pits.lua`, `script.lua`) in its extension and data folders and a `ext_car_controls.ini`. CSP reads such content through `FolderEncrypter::decryptFile` (hooked, see [`attach.md`](attach.md)) | Nothing: its data was **not** touched or decoded. Listed as "cannot be supported", as asked |

No other installed item is encrypted in the CSP sense. The `vrc_*_csp` cars' data decodes with the
ordinary AC key, but their **physics script** (`data/script.lua` inside `data.acd`) is scrambled and
was not touched. Their plain `extension/` Lua (visual, audio, displays, a signal-bus reader; 70 files
in three cars) was read by the Lua area: it calls 92 SDK functions and contains no physics write, but
it cannot run without the physics script's published layout (`lua.md` section 7.3).

## 3. Content that needs CSP physics (the part that decides "does it drive right")

`car.ini [HEADER] VERSION=extended-N` is the car's request for extended physics. Plain `acs.exe`
reads that value as a number and stops; rustyAC ignores it. **15 cars** carry it (earlier tasks
counted only the four that cannot load at all):

| Car(s) | `VERSION` | CSP physics data / features | Without CSP |
|---|---|---|---|
| `bdc_streetspec_350z_v4`, `_ae86_v4`, `_altezza_v4`, `_aristo_v4`, `_e36_v4`, `_miata_v4`, `_r32_v4`, `_rx7_v4`, `_s13_v4`, `_s14_v4` (10 drift mods, 134-344 MB each) | `extended-2` | ordinary DWB suspension data (no `_EXTENSION`); extended physics = CSP's extended tyre contact points and the experiment keys that need an extended car; visual `ext_config.ini` (extra lights, flames, Android Auto display, wipers) | **Loads in rustyAC** (it ignores VERSION). Plain AC refuses them. Drives with stock tyre contact points, so kerbs differ |
| `vrc_formula_alpha_2025_csp`, `vrc_formula_alpha_2026_csp`, `vrc_formula_beta_2024_csp` | `extended-2` | `suspensions.ini TYPE=COSMIC` (6 suspension definitions) and `[_EXTENSION]` (`TORQUE_MODE_EX`, `DAMPER_LUTS`, `FIX_PROGRESSIVE_RATE`), COILOVER / TORSION / ARB `EXTEND` keys, `brakes.ini [_EXTENSION] ENABLE`, `bumpstops.ini` / `dampers.ini [HEADER] ENABLE`, `engine.ini TURBO_0 EXT_GAS_CURVE / EXT_SPIN_DELAY`, `aero.ini MAP_0 MAP_BALANCE_RH`, `tyres.ini TYPE_HINT`, `colliders.ini` `STIFFNESS / MAX_WEAR / WEAR_COEFF / SPARKS_AS`, `setup.ini DJ4_LENGTH_OFFSET_* DISPLAY_MULTIPLIER`, `ext_car_controls.ini`, 19-28 Lua files (cphys script) | **Cannot run**: acs.exe and rustyAC refuse `TYPE=COSMIC` |
| `vrc_formula_lithium_2023_csp` | `extended-2` | DWB with `USE_DWB2=1` (CSP's own DWB), `_EXTENSION` in suspensions / tyres / brakes / colliders, `script.lua` | loads as plain DWB; extension keys ignored |
| `urd_darche_992_23` | `extendedV2-2` | encrypted | refused |

Two plain cars also carry CSP-only keys that plain AC ignores (`vrc_formula_alpha_2025`:
`ext_car_controls.ini`; `vrc_formula_lithium_2023`: `bumpstops.ini [HEADER] ENABLE`); they run in
plain AC and in rustyAC.

**Tracks:** six layouts have `surfaces.ini [SURFACE_0] WAV_PITCH=extended-0` plus `[_SCRIPTING_PHYSICS]
ALLOW_APPS=1`, which makes plain `acs.exe` stop at `std::stoi`: `ks_barcelona/layout_moto_fa_2026`,
`ks_red_bull_ring/layout_gp_fa_2026`, `ks_silverstone/gp_fa_2026`, `montreal/montreal_fa_2026`,
`monza/monza_fa_2026`, `spa/spa_fa_2026` (all added "FA 2026" layouts: 2 with `extension/`, none with
Lua). rustyAC refuses them today (`docs/port/tracks.md`). They need no CSP rendering; they need only the
`extended-N` parse (a one-line reader change) plus, optionally, track-script permission.

**Always-on physics changes** (every one of the 134 cars): the altitude **air-density law**, CSP's
**ODE build**, tyre contact points by Embree, collision fixes, AI changes: see
[`physics.md`](physics.md). The owner's grip investigation (`docs/port/grip_investigation.md`) is about
exactly these.

## 4. Content that needs CSP visual features

Number of installed items (cars + tracks) whose CSP config uses each family, by section names in
`ext_config.ini`, the downloaded `loaded/*.ini`, and the `common/` files they include:

| Feature family | Cars | Tracks | CSP module | Area file |
|---|---|---|---|---|
| Material adjustments, shader replacement, mesh adjustment, PBR carpaint / glass / interior (`MATERIAL_*`, `SHADER_REPLACEMENT`, `MESH_ADJUSTMENT`, `MODEL_REPLACEMENT`) | 19 | 12 | the new material system | [`rendering.md`](rendering.md) |
| Lights, emissives, light series, refracting headlights | 15 | 12 | Lighting FX | [`rendering.md`](rendering.md) |
| Exhaust flames, particles, sparks | 16 | 4 | Particles FX | [`rendering.md`](rendering.md) |
| Track atmosphere: conditions, SOL, wind, trees, occluders, VAO patches | 8 | 12 | WeatherFX, track adjustments | [`rendering.md`](rendering.md) |
| Displays: scripted displays, Android Auto, navigator, instruments | 12 | 6 | Car displays (Lua) | [`lua.md`](lua.md), [`ui_apps.md`](ui_apps.md) |
| RainFX, wipers, windscreen | 6 | 11 | RainFX | [`rendering.md`](rendering.md) |
| Animations, deforming parts, wobbly bits | 13 | 2 | Extra animations | [`rendering.md`](rendering.md) |
| Tyres FX, brake discs | 12 | 2 | Tyres FX | [`rendering.md`](rendering.md) |
| GrassFX | 0 | 12 | GrassFX | [`rendering.md`](rendering.md) |
| Audio tweaks (`AUDIO_VOLUME`, `AUDIO_PROPERTIES`, distant occlusion) | 10 | (generic: all Kunos) | CSP audio | [`audio.md`](audio.md) |
| Real / smart mirrors, local cubemaps, reflections | 7 | 2 | Smart Mirror, Reflections FX | [`rendering.md`](rendering.md) |
| Fake shadows, shadowed driver | 3 | 2 | Fake shadows | [`rendering.md`](rendering.md) |

RainFX has a second source: the DLL embeds 100 per-track `[RAIN_FX]` configs, and the rendering brief counts 19 of the 26 installed tracks as covered by them (`rendering_config.md`), so the RainFX row understates the tracks that get wet-road visuals.

All of these degrade gracefully in plain AC (unknown sections are ignored) and in rustyAC today;
the cost of not having CSP is **look only**.

## 5. Ranked list: which CSP feature unlocks the most

Ranked by (a) content that cannot load or drives wrongly without it, then (b) number of items touched.

| # | Feature | Items | What it unlocks | Port size |
|---|---|---|---|---|
| 1 | **CSP physics baseline** for stock cars: air-density law, double-precision ODE / contact handling, AI changes | all 134 cars | agreement with the owner's real AC + CSP laps (the 5-6 % downforce / drag at Spa, kerb contact) | S (air) to L |
| 2 | **`extended-N` parsing** (cars and track surfaces) | 15 cars + 6 layouts | 6 layouts and 10 BDC cars load in plain-AC form; a one-line change for tracks | S |
| 3 | **Extended-physics car data**: extra tyre contact points, `_EXTENSION` keys, aero maps, damper / bump-stop tables, brake hysteresis, DWB2 | 14 cars | physical agreement of the BDC drift cars and `lithium_2023_csp` | M-XL |
| 4 | **COSMIC suspension** | 3 cars (alpha 2025 / 2026, beta 2024 `_csp`) | the flagship VRC cars load at all | XL |
| 5 | **Lua car physics scripts** (LuaJIT on the physics thread) | 5 cars (4 VRC `_csp` + urd) | ERS, brake-by-wire, differentials of the VRC cars; urd is encrypted: out of reach | XL |
| 6 | **Config machinery**: `ext_config.ini` + INIpp (conditions, `$"..."` Lua expressions, `[INCLUDE]`, `[FUNCTION]`), generic / common / loaded configs | 27 cars, 14 tracks | prerequisite of every visual feature | M |
| 7 | **Materials / shader replacement** | 19 cars, 12 tracks | the look of most mod cars and CSP-tuned tracks | L-XL |
| 8 | **Lights, emissives and light series** | 15 cars, 12 tracks | headlights, tail lights, track lighting; also "true night" (brainstorm 2.17) | L |
| 9 | **WeatherFX + RainFX + wet roads** | every session; 11 tracks have a RainFX config | weather, night, rain grip and visuals | XL |
| 10 | Particles / flames / tyre smoke | 16 cars | flames and smoke | M |

Order for a port is in [`plan.md`](plan.md).

## 6. Notes and gaps

- `generic.ini` + `generic_fixes.ini` touch shaders only; nothing in them changes physics.
- `vrc_formula_lithium_2023` (plain) and `_csp` share `ext_config.ini`; the plain one drives in plain AC.
- Cars not owned (70 DLC placeholders) and the DLC track `ks_brands_hatch` contribute nothing.
- Which tracks the owner's `race.ini` picks are irrelevant here; configs for tracks that are not
  installed (downloaded by CM for `ks_*` tracks) were counted only when the folder exists.
- The three Kunos `fa_2026` layouts (Barcelona moto, Red Bull Ring, Silverstone) and the three mod ones
  (Montreal, Monza, Spa) differ in nothing else from their base layouts (`models_*_fa_2026.ini` + `data/surfaces.ini`) as far as the scan sees; their only CSP need is the parse above.
