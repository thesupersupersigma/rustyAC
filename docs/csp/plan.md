# CSP porting plan (proposal)

An ordered list of porting tasks of about 4-5 hours each, the way `docs/port/` was done for
`acs.exe`: what each delivers, how it is **proven**, what it depends on and what can block it.
Nothing here is built; every statement about CSP is in the area files this plan links to.
The ordering rule is the ranking of [`content_needs.md`](content_needs.md) section 5: first what
changes the physics of every owned car, then what lets the CSP content load, then the look.

Decisions that follow from the map (a short list for the owner):

1. **Do not port the mechanism.** CSP finds `acs.exe` functions by name through `acs.pdb`, detours
   them with an embedded MinHook-style library and keeps a per-car extension object
   ([`attach.md`](attach.md)). rustyAC owns the engine; "hooking" becomes ordinary options and
   per-car state in our own modules. What matters is the behaviour behind each seam.
2. **One exe, a run-time switch** (brainstorm 1.9a): every CSP behaviour is an option under a master
   `--csp`; CSP off stays Vanilla and bit-exact. Physics options are written into recordings.
3. **Prove statistically, not bit-exactly.** CSP replaces ODE with its own double-precision build
   (libccd, threads); there will be no bit-exact oracle for CSP physics. Acceptance is per-quantity
   tolerances against recordings. Pure functions (air density, config expansion, parse rules) can be
   exact.
4. Closed parts are out of scope: the VRC cars' physics scripts (scrambled inside `data.acd`),
   `urd_darche_992_23` (encrypted), obfuscated `patch.lua` / `user.lua`.

## 1. The proof method I recommend

CSP has no oracle yet. Three tiers, cheapest first; every task below names the tier it uses.

**Tier 1: A/B recordings of real AC with CSP on and off.** `general.ini [BASIC] ENABLED=0` turns CSP
inert, and rustyAC's Vanilla is already proven against plain `acs.exe`. So a CSP-on recording minus a
CSP-off recording of the same car, track, setup and inputs isolates exactly what CSP does, and rustyAC
`--csp` must reproduce the difference. Tools exist: `ac_telemetry.py` (shared memory), `compare_laps.py`,
`car_oracle` replays, `chassis_compare`. Used for: air density, always-on collision keys, AI changes,
tyre pressure law, session rules.

**Tier 2: reference data produced *inside* CSP.** CSP ships its own debug outputs; use them:
`custom_shaders_patch.log`; `[__DEBUG] DUMP_FLATTEN_INI` (the expanded `ext_config.ini`, the exact oracle for the
INIpp evaluator); the CSP Debug app physics tab; the **virtual K&C tests** (`knc.ini`, heave / roll /
lateral / longitudinal / MZ CSV output of COSMIC cars); and a small **Lua dump app** (and a tiny
open test car with a `script.lua`) that writes per-frame or per-physics-step state to a file through
the documented `ac.*` API. A test car with `VERSION=extended-1` and an open script gives a 333 Hz
reference of CSP's physics for known inputs, with no dependence on the encrypted VRC scripts.
Used for: INIpp, COSMIC, extended data, tyre model, Lua API behaviour.

**Tier 3 (spike, may fail): CSP in the oracle process.** The existing oracles load `acs.exe` in
process. Whether `dwrite.dll` can be loaded the same way (it uses `GetModuleFileNameW` and the main
module, and creates D3D11 devices) is an open question ([`attach.md`](attach.md) section 8, item 6).
If it works, hook slots give per-function call logs (CSP's own functions run with `acs.exe`'s data)
and replays of `car_oracle` tapes under CSP. If it does not, tiers 1-2 are enough.

Static proof, no game needed: every constant decoded from the DLL (air-density law, record layouts)
is checked by unit tests; `memory_layout.bin` (6,280 names, 29,142 `(RVA, size)` pairs) can be diffed
against `re/index/functions.tsv` to prove the name -> function map this atlas used.

## 2. Task list

Sizes are hours of work for one session. "Needs recording" means the owner has to drive real
AC + CSP once (a few laps); nothing else needs the owner at the keyboard.

| # | Task | Delivers | Proof (tier) | Needs | Size |
|---|---|---|---|---|---|
| **CSP-0** | **Reference kit and the in-process spike** | `csp_ref/` kit: a Lua dump app (`apps/lua`), a test car with open `script.lua`, a recording script for A/B runs; a harness that tries `LoadLibrary(acs.exe)` with `dwrite.dll` next to it and reports whether CSP's "All N functions were found" appears | tier 3 spike result (go / no-go), tier 1/2 recordings: F2004 Spa 3 laps CSP on/off, a stationary altitude run on 3 tracks | owner drives once | 4 h |
| **CSP-1** | **CSP switch, `extended-N` parse, feature flags** | `--csp` master switch + per-car / per-track `CspFeatures` (flag, N, `[_EXTENSION]` presence); `VERSION=extended-N` / `extendedV2-N` and `WAV_PITCH=extended-N` accepted exactly as CSP's `INIReader` hooks do; `[_SCRIPTING_PHYSICS]` keys parsed; the 6 `*_fa_2026` layouts and the 10 BDC cars load under `--csp`; `urd_darche_992_23` and COSMIC cars still refused with the right reason | `acd_check` / `track_survey` tables before / after (tier static); load-all dry run lists exactly 6 layouts + 14 cars newly accepted | none | 3 h |
| **CSP-2** | **CSP physics baseline, S part** | air density law (barometric, near-ground temperature blend, pit-altitude table of ~30 Kunos tracks + surfaces.ini key), engine thread-local density, `ABS::isInAction` redefinition, brake temps when the AI drives car 0, `FIX_SURFACE_TYPES` prefix fallback, 1 ms transponder, 250-step AI sleep, `getPhysicsState` low-pass | tier 1: `airDensity` per row vs `f2004_spa_ai.csv` (law reproduces it to 0.03 K), engine accel at altitude, AI-lap rows | the 0xaf7010 helper decompile (request in `physics_powertrain_aero.md`) | 4 h |
| **CSP-3** | **INIpp and `ext_config.ini` machinery** | LuaJIT (`mlua`) host for config only; `[INCLUDE]`, `ACTIVE=$"..."`, `[FUNCTION:]`, `@GENERATOR`, templates, `read()`/`get()`/`has()`, conditions; generic / common / loaded / car / track config resolution order; unknown sections kept for later modules | tier 2: `DUMP_FLATTEN_INI` of the same ini under real CSP == our expansion, key by key (725 expressions in 43 files here) | CSP-0 recordings of the flatten dumps | 5 h |
| **CSP-4** | **Collision and contact experiments** | `ENABLE_MESH_COLLIDER_SOONER`, `CLAMP_MESH_COLLIDERS`, `EXTEND_COLLIDER_BOXES`, `ONE_SIDED_BOXES`, rim collision, smooth / adaptive collisions, `ALT_COLLISIONS_*`, extra contact points, closest-hit ray rule for overlapping surfaces; offline-only gate | tier 1: kerb sequence at Spa Bus Stop, each key 0/1: `wheelLoad[4]`, `suspensionTravel[4]`, `tyreContactPoint`; replaces the earlier "R3" run | recording per key | 5 h |
| **CSP-5** | **AI behaviour changes** | the `new_behaviour.ini [AI_TWEAKS]` set: lateral-grip estimate, grip multiplier assist, cautious steering, lookahead, shift thresholds, alternative optimal brake, smoothing, race cooldown, wrong-way rules | tier 1: AI laps on 2 tracks (CSP on/off): speed / gas / brake / gear / steer per row; corner speeds and shift points | CSP-2 baseline | 5 h |
| **CSP-6** | **Double-precision physics option** | `rustyac-ode` generic over `f32`/`f64`; CSP's constants (gravity -9.806, 0.001 m contact layer), thread-free; `--csp-ode f64` | tier 1 + tier 2: settle test, drop test, 60 s replay: RMS of `velocity`, `wheelLoad`, `suspensionTravel` against the test car recording; thresholds fixed in advance | CSP-0 test car | 5 h |
| **CSP-7** | **CSP's tyre step for stock data** | the places where `Tyre::step` differs for stock cars: gas-law pressure, brake-torque clamp, random initial wheel angle, damage only grows, wet-grip law, `getCorrectedD`, curve clamp | tier 1: pressure and temperature rows of CSP-on vs CSP-off laps; static unit tests for each rule | decompiles listed in `physics_tyres_suspension.md` | 5 h |
| **CSP-8** | **Extended car data** | dampers with LUTs / blow-off, bump-stop tables, brake hysteresis / `[_EXTENSION]`, aero maps `[MAP_n]`, turbo `EXT_GAS_CURVE` / `EXT_SPIN_DELAY`, `colliders.ini` wear, `ext_car_controls.ini`, DWB2 | tier 2 (`accessCarDamper` dumps, CSP Debug aero panel) + tier 1 on `vrc_formula_lithium_2023_csp`, one BDC car | CSP-1, CSP-3 | 5 h |
| **CSP-9** | **COSMIC suspension, part 1: model** | bodies, joints (ball, distance, hinge, steering, slider, rack), spring / coil-over / torsion / damper elements, cphys ARB, static K&C solver | tier 2: CSP's virtual K&C CSVs for `vrc_formula_alpha_2026_csp` | CSP-6; decompile of loader `0x87f080` | 5 h |
| **CSP-10** | **COSMIC part 2: dynamics and the three `_csp` cars** | joint motors, flex, setup bindings (`DJ4_LENGTH_OFFSET_*`), all three Formula `_csp` cars running without scripts (scripts blocked, see below) | tier 1/2: ride height at rest, heave / roll, kerb run vs real CSP | CSP-9 | 5 h |
| **CSP-11** | **Lua host S1-S3: types, state read, data** | `vec/rgb/hsv/quat/mat`, `class`, `stringify`, `ac.getSim/getCar/getCarPhysics`, `ac.INIConfig`, `DataLUT`, `ac.store/connect/StructItem` over `ac.StateCar` views | tier 2: the same Lua dump app run under CSP and under rustyAC, compared per frame; golden vectors | CSP-3 | 5 h |
| **CSP-12** | **car physics script host (open scripts)** | `script.lua` on the physics thread (`update(dt)`, `reset()`), `ac.override*`, `ac.addForce`, `SCRIPT_n` controller inputs, `physics.allowed()`; run on the test car and on any open third-party script | tier 2: test car logs at 333 Hz vs CSP; VRC cars **blocked** (scrambled scripts) | CSP-6, CSP-11 | 5 h |
| **CSP-13** | **Weather and true night** | astronomy (sun / moon directions, phase), unclamped sun angle, night colour table, moon light, stars sky layer, exposure; weather type files and the `base` controller | image oracle: AC + CSP screenshots at fixed `ac.setWeatherTimeOffset` times (tier 2); `render_oracle` compare against the CSP shots | CSP-3, CSP-11 | 5 h |
| **CSP-14** | **Materials and lights from `ext_config.ini`** | `MATERIAL_ADJUSTMENT`, `SHADER_REPLACEMENT`, `MESH_ADJUSTMENT`, `LIGHT_*`, `EMISSIVE_*`, light series, adaptive emissives for the installed cars and tracks (19 cars, 12 tracks) | image oracle as above, per car at fixed camera; CSP shaders are public, the DXBC archive can be used | CSP-3, CSP-13 | 5 h x2 |
| **CSP-15** | **RainFX and wet roads** | rain intensity, puddles, wet grip law hookup (CSP-2/7), drops, wipers, per-track configs | tier 1 for grip; image oracle for visuals | CSP-13 | 5 h |
| **CSP-16** | **Audio deltas** | second GUID table, `csp_*` banks, volume chain, reverb forcing, per-event extension, RainFX / surface sounds | `audio_oracle` FMOD call-log compare (existing) | CSP-3 | 5 h |
| **CSP-17** | **Lua apps and display scripts** | manifest loader, `ui.*` subset, scripted displays (Android Auto), the 4 installed apps | screenshot compare of the apps | CSP-11 | 5 h x2 |
| **CSP-18** | **Online compatibility** | what a client must add to join CSP servers (extra packets, checksums, `[EXTRA_RULES]`) | packet capture against a real AC + CSP server | online brief | 5 h |
| **CSP-19** | **Input / FFB, VR, misc** | gamepad script, DualSense, FFB post-processing; VR paths; rest | per area file | the area files | later |

Notes on order: CSP-0 and CSP-1 first (the first decides the proof path, the second unlocks 6 layouts
and 10 cars at once for 3 hours). CSP-2 next: the owner's grip question is mostly this. CSP-3 is the
prerequisite of everything content-visual and of the Lua host. CSP-9/10 are the largest single unlock
(three flagship cars) but cannot be finished without the closed scripts for full behaviour.

## 3. What it would take

- **Time**: CSP-0 to CSP-8 (physics baseline and CSP-content data) is about 40 hours; COSMIC +
  Lua scripts another 25; weather, materials, rain, audio, apps about 40-50 more. A useful
  "CSP cars drive right" milestone is CSP-0..CSP-8 + CSP-9..10.
- **Decompiles still wanted** (listed per area file under "Decompile requests"): the near-ground
  temperature helper `0xaf7010`; CSP's `Tyre::step` bodies (`0x69c4f0`, `0x6b4170`, `0x6b2e90`); the
  tyre loader `0x8cc0e0`; COSMIC loader `0x87f080`; the car-script caller chain (`0x856ed0`,
  `0x1373420`, `0x1375740`); `Car::pollControls` double registration; audio helpers. About 150 RVAs in
  all, several hundred KB of pseudo-C.
- **Tools**: a LuaJIT build for Rust (`mlua` with vendored LuaJIT; no wasm path, so CSP stays a
  native-only feature), a per-key A/B recording workflow, the test-car pack.

## 4. Blockers and risks

1. **Closed physics scripts.** The three Formula `_csp` cars and `lithium_2023_csp` run their ERS,
   brake-by-wire, differential and DRS logic in a scrambled `script.lua`. rustyAC can load the
   data but not reproduce the behaviour. Only the open parts (visual Lua, data) are portable.
2. **`urd_darche_992_23`** is encrypted for CSP: cannot be supported.
3. **No bit-exact oracle** for CSP physics (double-precision ODE with threads). Acceptance is
   statistical and must be written down per task before the recording is taken.
4. **A moving target.** CSP changes every few weeks; the plan pins preview649 / build 4184 and the
   `acs.pdb` of 1.16.4. A different build means a different seam list.
5. **Hidden keys.** About 100 DLL key names and many CSP-only `tyres.ini` / engine keys have no plain
   text in the DLL (hashed or obfuscated); some are reachable only from recordings.
6. **Tier 3 may be impossible** (CSP needs the real process for `DllMain` and creates its own D3D
   devices); then all CSP reference data comes from real AC runs the owner makes.
7. **The browser build** has its own renderer and no LuaJIT: CSP features would be native-only.
8. **Licensing and copying.** The Lua SDK, config files and the public shader sources are
   documented and public; CSP's code is not copied. Re-implementation stays in our own words and
   structure, as this atlas does.
