# Rendering: graphics modules, WeatherFX, lighting, materials, RainFX (configuration side)

CSP's visual half is a set of about 25 ini-configured modules (one `extension/config/<module>.ini` each), a Lua
"WeatherFX" layer (a controller script plus a style script that drive the sky, light, fog, clouds and rain), a
material / shader-replacement / light language (INIpp: `[SHADER_REPLACEMENT_n]`, `[MATERIAL_ADJUSTMENT_n]`,
`[LIGHT_SERIES_n]`, `[CONDITION_n]`, ...) read from per-car and per-track configs, and about 6,700 compiled
shaders inside `dwrite.dll`. This file maps the **configuration side**: what each module is, which keys drive it,
how the sky and "true night" work, what lights and materials can express, and what rustyAC would need. The
hook side (which acs.exe renderer functions are replaced) is the `rendering_hooks` brief; I only cross-reference it.

## 1. What it does

What the player sees, by module (all `[ini]` unless marked):

- **Sky, sun, moon, stars, clouds, fog, time of day (WeatherFX).** A Lua "style" (`extension/weather/base`) computes
  the sun and moon from date, time, latitude, longitude and time zone, and sets light, ambient, sky, fog, clouds,
  stars, auroras, lightning, rain haze and the exposure through `ac.set*` calls. A Lua "controller"
  (`extension/weather-controllers/{base,base-live,base-schedule}`) decides the weather type, rain and wetness.
  Time may run through the whole day and night; the vanilla clamp of the sun at +/-80 degrees is gone.
- **Dynamic lights (Lighting FX).** Real point, spot and line light sources from car headlights, brake and reverse
  lights, track lamps and extra lights; bounced light; dynamic ambient; optional shadows from lights.
- **Materials.** New shaders (PBR, car paint with clear coat and flakes, glass, interior, water, digital screens,
  heating glow) selected by name patterns and adjusted per material, with conditions (night, flag colour, ...).
- **Shadows.** Smart Shadows (4th cascade, stabilised and smooth cascades), fake shadows under cars (projected with
  Extra FX), coloured shadows through coloured glass, shadowed wheel arches.
- **Rain and snow (Rain FX).** Falling drops, hits, splashes, puddles, wet roads, streams along kerbs, a racing line
  that dries, drops on the screen, the windscreen with wipers, wet cars, rain audio. Snow replaces the drops.
- **Extra FX.** A second "G-buffer" pass: TAA, screen-space reflections, ASSAO / HBAO+, screen-space lights, light
  bounce, volumetric headlights, fog blur, motion blur, modern upscalers. **Off by default** and needs post-processing.
- **Particles, grass, skid marks, tyres.** GPU smoke, sparks, flames, solid pieces, fireworks, marbles; procedural
  grass; new skid marks including dirt; tyre flex, flat spots, dirt, brake disc wear.
- **Picture pipeline.** Render scale, six upscalers, CMAA / SMAA / FXAA, new tone mappers, a lightweight YEBIS
  replacement, scripted post filters, 360 / fisheye modes, accumulation screenshots.

On the owner's PC (one race on `monaco_2017`, log `custom_shaders_patch.log`) most of this is **idle**: his
`video.ini` has `[POST_PROCESS] ENABLED=0`, `CUBEMAP SIZE=0`, `SHADOW_MAP_SIZE=-1`, `WORLD_DETAIL=0`, so Extra FX,
linear colour space and DXGI tweaks do not run (section 5).

## 2. Game parts it replaces or extends

All hook rows below are from `hook_groups.tsv` / `csp_hook_table.tsv` (acs.exe VA, replacement RVA in
`dwrite.dll`, image base 0x180000000). The renderer group in `hook_groups.tsv` has **368 rows: 301 hooks, 67
lookups** (of 1,441 registration sites). Per-class counts are in `attach.md` section 2.2; this table only lists the
ones that carry the configuration features. `[dll]`

| Feature | acs.exe symbol (VA) | CSP replacement RVA | What it does there |
|---|---|---|---|
| Sun | `SunAnimator::update` (0x1401adc10) | 0x1121f60 | walks a handler chain (vector at 0x1826974c8, 0x28-byte entries, last registered first); if a handler sets "handled" the original (slot 0x18268c3f0) is not called |
| Sun | `SunAnimator::setSunDirection` (0x1401adbd0), `enableCustomSunDirection` (0x1401adab0), ctor (0x1401ad890) | 0x1121ec0, 0x1121dc0, 0x1121d80 | keeps CSP's own sun record in step; custom sun direction can be switched on and the original is skipped when a flag is set |
| Weather | `WeatherManager::applyCustomWeather` (0x1401d8110), ctor (0x1401d8020) | 0x112f9a0, 0x112f960 | stores the weather folder name; skips the original when CSP's weather path is active |
| Sky | `SkyBox::render` (0x14021d0d0), ctor (0x14021c790), `updateCloudsGeneration` (0x14021db00) | 0x11c5670, 0x11c5630, 0x11c5850 | render handler chain (vector at 0x182697a30) before the original; fills four cbuffer rows |
| Lighting | `GraphicsManager::updateLightingSetttings` (0x140205190) | 0x117f630 | **calls the original first, then marks the lighting dirty (gm+0x478) and runs a post-update chain (vector at 0x182697570)**: CSP edits vanilla's result, it does not replace the curves |
| Frame | `GraphicsManager::beginScene` (0x140202580), `beginMainRenderPass` (0x140202570), `commitShaderChanges`, `setShadowMapBias` etc. (36 rows) | 0x117f2d0, 0x117eb20, ... | beginScene is re-implemented with direct context calls, then runs the same lighting chain |
| LDR | `GraphicsManager::getLDRScale` (0x140202bb0) | 0x414fc0 | returns the constant 1.0 (lamps are no longer scaled by the HDR-off factor) |
| Static cube map | `Sim::initStaticCubemap` (0x14019a2a0) | 0x11bd8b0 | logs "WeatherFX is present, no need to render cubemap" and skips the draw |
| Shadows | `CameraShadowMapped::{ctor, createShadowMapMatrix, setShadowMapsSplits, shadowMapPass}` (0x14020bdd0, 0x14020c790, 0x14020d5f0, 0x14020d6a0) | 0x10a5ef0, 0x10a5c10, 0x10a5fa0, 0x10a6060 | Smart Shadows: splits, 4th cascade, stabilised matrices |
| Post | `CameraForwardYebis::{ctor, initPostProcessing, initRenderTargets, updateYebisParameters, applyPostProcessing, render, renderApplyEffect, loadPPSet, readPPSetOptions}` (9 hooks), `YebisPP::*` (6 hooks, 2 lookups), `PostProcessEffectsUpdater::{ctor, update}` | 0x109f610 ... 0x10a0ff0; 0x11cd750 ... 0x11cdf70; 0x6915a0, 0x6915c0 | the YEBIS frame is wrapped so that CSP's passes (alternative tone map, upscalers, scripted filters) can run around it |
| Reflections | `CubeMap::{ctor, apply, beginFace, endFace, generateMips}` (5), `CubeMapRenderer::render` (0x14021edb0) | 0x10b4c40 ... 0x10b5230, 0x10b7240 | per-face scheduling, reprojection, prefiltering |
| Mirrors | `MirrorTextureRenderer::{ctor, render}`, `VirtualMirrorRenderer::*`, `CarMirrorManager::*` | 0x1195780, 0x1195df0; 0x11cd160, 0x11cd1a0; 0x10b04d0, 0x10b0720 | real mirrors, post-processed virtual mirror |
| Fake shadow | `CarFakeShadow::{ctor, onNodeRenderEvent}` (0x1400e0d80, 0x1400e22b0), `generateFakeShadow` (0x1400e1430) | 0x5a3f30, 0x5a4c60, 0x2db1f0 | replacement fake shadows; the cached shadow is rebuilt with baking |
| Particles | `ParticleSystem::*` 3 hooks, `StaticParticleSystem::*` 3, `Flames::*` 8, `TyreSmoke::*` 2, `SkidMarkBuffer::{ctor, render}`, `KGLYebisParticleManager::setupPfxParticles` 2 | 0x10e0e90 ..., 0x1110390 ..., 0x10989b0 ..., 0x11cc1e0, 0x6a0710, 0x60a490, 0x60a8a0 | new GPU particle systems take over |
| Car visuals | `BrakeDiscGraphics` 2, `BlurredObjects` 2, `TyreBlur` 2, `DynamicCarEffects` 2, `CarBrakeLights` 2 hooks + 1 lookup, `AnimatedLights` 2, `FuelLight` 2, `DriverModel` 4, `VisualDamageManager` 3 | 0x58e0f0 ..., 0x58d620 ..., 0x6b8c70 ..., 0x5bff90 ..., 0x10910b0 ..., 0x10e1ee0 ... | brake disc shader, tyre damage, lights |
| Materials | `Material::{ctor x2, getVar, createCBuffers, setShader, initShaderVars, apply}` 8 hooks, `KN5IO::{load, loadMaterialsBinary, loadBinaryV2, loadTexture, getSkinOverridenTexturePath}` 8 hooks, `Mesh`, `Node`, `SkinnedMesh`, `CarLodManager` 4, `TrackAvatar::init3D` (0x1401c8740) | 0x11258b0 (init3D) | shader replacement, material adjustments, model insertion are applied while the model is loaded |

Module settings are C++ classes `ac_ext::ac_ext_<module>` (40 such RTTI names) with `ac_ext_<module>_settings`
structs (47); the list of hooks CSP installs is the same for every user, so every module is "on" at the hook
level and decides at run time (`attach.md` 1.7). `[dll]` RVAs of the class-name strings: `ac_ext_weather_fx`
0x262cc40, `ac_ext_reflections_fx` 0x26252d0, `ac_ext_smart_shadows` 0x262d288, `ac_ext_skidmarks_fx` 0x2625a88.

## 3. Config keys and Lua API

### 3.1 The module files (one table)

Source: `<AC>/extension/config/*.ini`. `keys` = uncommented `KEY=` lines I counted; `~hidden` = lines whose comment or
name marks them hidden (approximate). Owner = `Documents/Assetto Corsa/cfg/extension/<file>`. Sizes are bytes.

| File | FULLNAME | State by default | keys / ~hidden / commented / sections / bytes | Owner's override |
|---|---|---|---|---|
| `lighting_fx.ini` | Graphics: Lighting FX | on (`[BASIC] ENABLED` is a documented no-op); `[SHADOWS] ENABLED=0` | 64 / 35 / 8 / 6 / 4,852 | `[BASIC] ENABLED=0` (a no-op by the file's own comment) |
| `weather_fx.ini` | Graphics: Weather FX | on, `CONTROLLER=sol`, `IMPLEMENTATION=base` | 33 / 2 / 0 / 10 / 3,381 | file exists, empty |
| `rain_fx.ini` | Simulation: Rain FX | on | 29 / 1 / 6 / 6 / 3,036 | `[VISUAL_TWEAKS] RAIN_MAPS_QUALITY=2` (Low) |
| `reflections_fx.ini` | Graphics: Reflections FX | on (no-op flag) | 17 / 2 / 7 / 9 / 2,157 | none |
| `extra_fx.ini` | Graphics: Extra FX | **off** (`ENABLED=0`) | 83 / 29 / 3 / 13 / 5,927 | none |
| `particles_fx.ini` | Graphics: Particles FX | on | 116 / 15 / 3 / 19 / 7,728 | `[BASIC] ENABLED=0` |
| `grass_fx.ini` | Track: Grass | on, `QUALITY=2` | 53 / 0 / 1 / 10 / 1,662 | `[BASIC] QUALITY=0` (Very low) |
| `smart_shadows.ini` | Graphics: Smart Shadows | on | 33 / 3 / 6 / 8 / 4,454 | none |
| `fake_shadows_fx.ini` | (none) | no-op flag; real keys are `tyres_fx.ini [FAKESHADOWSFX_*]` | 1 / 1 / 0 / 1 / 109 | `[BODY_SHADOW] USE_PROJECTION=0` (a section that does not exist in the defaults: no effect [interp]) |
| `colorful_shadowing.ini` | (none) | no-op flag | 1 / 1 / 2 / 3 / 472 | `[BASIC] ENABLED=0` (no-op) |
| `skidmarks_fx.ini` | Track: Skidmarks | on, new implementation | 21 / 2 / 1 / 6 / 2,111 | none |
| `tyres_fx.ini` | Cars: Visual | on (tyres, brake discs, shadowed wheels, fake shadows, extras) | 33 / 6 / 7 / 10 / 3,865 | none |
| `windscreen_fx.ini` | Cars: Windscreen FX | on | 19 / 1 / 0 / 6 / 1,833 | `[BASIC] ENABLED=0` |
| `brakedisc_fx.ini` | (none) | no-op flag; real key `tyres_fx.ini [BRAKEDISC_FX]` | 1 / 1 / 0 / 1 / 109 | `[BASIC] ENABLED=0` (no-op) |
| `shadowed_wheels.ini` | (none) | no-op flag; real key `tyres_fx.ini [SHADOWED_WHEELS]` | 1 / 1 / 0 / 1 / 109 | none |
| `graphics_adjustments.ini` | Graphics: Adjustments | always on ("disabling might break most of CSP") | 140 / 18 / 15 / 22 / 22,802 | none |
| `custom_rendering_modes.ini` | Display mode: custom | on only when a custom mode is selected (log: "custom mode is not selected") | 16 / 5 / 0 / 4 / 1,698 | none |
| `yebisest.ini` | (none) | **obsolete**: "This config is no longer doing anything" | 0 / 0 / 0 / 0 / 41 | none |
| `dxgi_tweaks.ini` | Other: DXGI | on; log: "disabled: not available with MSAA without post-processing" | 37 / 11 / 9 / 7 / 8,332 | none |
| `nice_screenshots.ini` | Other: Screenshots | on | 59 / 16 / 0 / 10 / 6,621 | none |
| `track_adjustments.ini` | Track: Miscellaneous | on (no-op flag) | 27 / 9 / 9 / 8 / 3,583 | `[BASIC] ENABLED=0` (no-op) |
| `smart_mirror.ini` | Cars: Mirrors | on (no-op flag); `REAL_MIRRORS` off | 72 / 5 / 2 / 17 / 5,972 | none |
| `car_instruments.ini` | Cars: Instruments | on | 31 / 0 / 6 / 5 / 3,065 | none |
| `small_tweaks.ini` | Other: Small tweaks | on | 73 / 10 / 1 / 15 / 8,556 | none |
| `surfaces_fx.ini` | Simulation: Surfaces FX | **off** (`ENABLED=0`, needs custom physics) | 17 / 3 / 0 / 4 / 1,333 | none |
| **25 files** | | | **977 / 177 / 201 / 103 / 103,808** | 9 files override (4 effective, 4 documented no-ops, 1 unmatched section) |

Notes. (a) Content Manager writes only keys that differ from the default, so `weather_fx.ini` being empty means
"all defaults". (b) Three owner overrides are documented no-ops (`lighting_fx`, `brakedisc_fx`,
`colorful_shadowing`, `track_adjustments` `[BASIC] ENABLED`): Lighting FX, brake disc FX and track adjustments are
therefore **still on** on his PC; only Particles FX, Windscreen FX, grass quality and rain-map quality really changed.
(c) There are 36 module ids (`ac.CSPModuleID`): the four "no-op flag" files above are not in the enum; they are
folded into `tyres_fx` ("Cars: Visual"). `[sdk]`

### 3.2 The 10 most important keys per module (default)

- **lighting_fx**: `[BASIC] BOUNCED_LIGHT_MULT=0.35`, `BOUNCED_INTERIOR_LIGHT_MULT=0.6`, `HEADLIGHTS_RANGE_MULT=1`,
  `HEADLIGHTS_SPOT_MULT=1`, `DYNAMIC_AMBIENT_SATURATION=0.8`, `TRACK_AMBIENT_GROUND_MULT=0.4`, `EMISSIVE_CAMERA_GAIN=5`;
  `[PERFORMANCE] CARS_WITH_LIGHTS=10`, `ENABLE_TREES_LIGHTING=1`, `ENABLE_REARVIEWMIRROR_LIGHT=1` (0 none, 1 lights,
  2 lights and sun shadows), `ENABLE_REFLECTION_LIGHT=1`; `[SHADOWS] ENABLED=0`, `CARS_SHADOWS=2`,
  `SPECTATING_CARS_SHADOWS=3`, `DETAILED_SHADOWS_FROM_CARS_NEARBY=1`; `[RANGES] COMPLEXIFY_RANGE_FROM/TO=50/500`,
  `NOMIRROR_FROM/TO=120/150` (distance at which paired headlights collapse into one). Needs: nothing (works without
  post-processing); light shadows and volumetric lights need Extra FX for the volumetric part.
- **weather_fx**: `CONTROLLER` (script in `extension/weather-controllers`), `IMPLEMENTATION` (script in
  `extension/weather`), `[PERFORMANCE] USE_COMPATIBLE_SKY_SHADER=1`, `HIGH_QUALITY_STARS=1`,
  `EXTRA_CLOUDS_FIDELITY=1`, `ADDITIONAL_EFFECTS=1`, `DETAILED_CLOUD_SHADOWS=1`; `[MISCELLANEOUS]
  RESET_TIME_ON_SUBSEQUENT_SESSIONS=1`, `SWITCH_WIPERS_WITH_AI=1`, `FORCE_HEADLIGHTS=0`,
  `SWITCH_HEADLIGHTS_WITH_AI=0`; `[STATIC_REFLECTIONS] OCCASIONALLY_REFRESH=1`, `REFRESH_FACE_DELAY=2.5`;
  `[TWEAKS] INCREASE_CONDITIONS_LIMITS=1` (wind above 35 km/h, temperature outside 0..36 C),
  `APPLY_AUTOMATIC_WHITE_REF_POINT_FIX=1`; `[NOISE_VOLUME] RESOLUTION=512` (a 512x512x32 volume, "45 MB of VRAM").
  Needs: post-processing for linear colour space (style setting) only.
- **rain_fx**: `AUTOSELECT_RAIN_TYRES=1`, `[VISUAL_TWEAKS] RAIN_MAPS_QUALITY=1` (0 High, 1 Medium, 2 Low),
  `HITS_INTENSITY=1`, `SNOW_MODE=1`; `[SCREEN_DROPS] ENABLED=1`, `INTENSITY_MULT=1`, `STYLE_COCKPIT=DISTANT`,
  `STYLE_CHASE_CAMERA=CLOSE`, `STYLE_TRACK_CAMERA=NONE`; `[WINDSCREEN_DROPS] GFORCE_MULT=1`; `[RACING_LINE_DEV]
  FADING_PERIOD=600` (s), `SPEED_THRESHOLD=10,90`. Needs: per-track rain config (embedded, section 3.7), compute
  shaders. Commented keys show the sub-modules: relief map, drop maps, puddle maps, car wetness (+ CPU sync), drops.
- **reflections_fx**: `[MAIN_CUBEMAP] RESOLUTION2=512` (1024, 2048 "not recommended"), `REPROJECT_PARTIAL=1`
  (one face per frame feels like six), commented `USE_PROPER_IBL2`, `USE_INTERIOR_MASK2`, `PREFILTER_CUBEMAP2`,
  `BATCH_PROCESSING2` (all 1); `[TWEAKS] CUBEMAP_FOR_FOCUSED_CAR=0` (0 default, 1 focused car, 2 ground);
  `[REFRACTING_HEADLIGHTS2] ENABLED=1`, `CACHE=1`; `[LOCAL_CUBEMAPS_CARS2]` and `[LOCAL_CUBEMAPS_TRACKS2]`
  `ENABLED=1`, `ALLOW_DYNAMIC=1`; `[MAIN_CUBEMAP_DEV] USE_64BPP=0`.
- **extra_fx**: `[BASIC] ENABLED=0`, `ALLOW_IN_TRIPLE=0`, `EXTERIOR_ONLY=0` (0 always, 1 exterior, 2 replays);
  `[TAA] ENABLED=1` `SAMPLE_SPREAD_MULT=1` `EXTRA_SHARPEN_PASS=0.5`; `[SSLR] ENABLED=1` `TRACING_QUALITY3=3` (0..2 old
  low / medium / high, 3 "Simple (New)", 4 "Hi-Z (New)") `STEPS_SIMPLE=36` `STEPS_HIZ=80` `EXTRA_SPECULARS=1`;
  `[ASSAO] ENABLED=0` `QUALITY=2`; `[HBAO] ENABLED=0`; `[SS_LIGHTING] ENABLED=1` `SCALE=1` `EMISSIVE=1`;
  `[LIGHT_BOUNCE] ENABLED=1` `CARS=1`; `[VOLUMETRIC_LIGHTS] ENABLED=1` `SCALE=0.2` `INTENSITY_MULT=1.5`;
  `[FOG_BLUR] ENABLED=1` `HEATING=1`; `[MOTION_BLUR] ENABLED=0` `MULT=0.5` `QUALITY_2=1`. The module's own
  description: "only works without VR, with post-processing enabled. Also, doesn't work if AC's motion blur is
  enabled". Hidden: `[SSAO] ENABLED2=0`, `[SSGI] ENABLED2=0` ("Obsolete and incorrect").
- **particles_fx**: `[AC_PARTICLES] SOFT_PARTICLES=1`, `SORTED_PARTICLES=0`, `REWORKED_HEAT_SHIMMER=1`; `[SPARKS]
  LIMIT=40000` `SPAWN_RATE=1000`; `[SMOKE] QUANTITY_SCALE=1` `SIZE_LIMIT_SCALE=0.8` `LONG_LASTING_SMOKE=1`
  `FILL_SCREEN=1` `CAST_SHADOWS=1` `SHOW_IN_MIRRORS=1`; `[FLAMES] USE_DYNAMIC_SHADOWS=1`; `[FIREWORKS]
  BEHAVIOUR=2` (0 script only, 1 always, 2 holidays, 4 after finish, 8 night) `IMPLEMENTATION=holidays`;
  `[PIECES] LIMIT=60000`; `[MARBLES_2] ENABLED=0`; `[TRACES] ENABLED=0` ("Initial D braking trails"). Hidden
  budgets: `SMOKE_DEV LIMIT=8000`, `MIRAGEDISRUPTION 4000`, `CARDROPS 8000`, `SKIDWET 10000`, `OILSPILL 100`,
  `WINDSCREEN 100000`; `[SPARKS_DEV]` has 24 physics constants (drag, gravity -9.81, lifespan formula, wheel and
  rear-of-car forces).
- **grass_fx**: `QUALITY=2` (0..4), `SKIP_LOADING_OLD_MESHES=1`; `[RENDERING] SMOOTHER_BLENDING=0`, `CAST_SHADOWS=0`,
  `EXTRAFX_PASS=0`; hidden `[GENERATION_0..4]` (`MAP_RESOLUTION` 512 / 1024, `MAP_STEP=0.25`, `PASS_n=<density>,
  <step>`), `[DEFORMATION] MAP_RESOLUTION=1024 MAP_RADIUS=100`, `[AIR] 256 / 50`.
- **smart_shadows**: `AUTOMATIC_SPLITS=1`, `AUTOMATIC_SPLITS_DISTANCE=200` (50..400 m), `AUTOMATIC_SPLITS_LAMBDA=0.9`,
  `INTERIOR_SPLIT=1.3,40,200` (Kunos: 1.3, 80, 250), `EXTERIOR_SPLIT=10,50,250` (Kunos: 5, 25, 250),
  `FIRST_SPLIT_ADAPTIVE2=1`, `BIASES=0.04,0.25,1,4`; `[SMOOTH_CASCADES] ENABLED=1 OVERHANG_MULT=0.95`;
  `[CUSTOM_SHADOW_MATRICES] ENABLED=1 LOOKLESS=2 FOURTH_CASCADE=1 FOURTH_CASCADE_DISTANCE=1500`; `[LAZIER_UPDATE]`
  (two of three maps per frame); `[WITHOUT_SHADOWS] SHADOWED_INTERIOR_MAIN=1`.
- **tyres_fx** (Cars: Visual): `[BASIC] ENABLED=1` ("visually deforming and damageable tyres"); `[BRAKEDISC_FX]
  ENABLED=1 INITIAL=0.4 INTENSITY=1`; `[SHADOWED_WHEELS] ENABLED=1`; `[FAKESHADOWSFX_BAKING] ENABLED2=1
  LIGHTS_COUNT=600`; `[FAKESHADOWSFX_LOOK] OPACITY=1 CONCENTRATION=0.5`; `[FAKESHADOWSFX_LOOK_LIGHT]
  SHADOW_FACTOR=0.4`; `[FAKESHADOWSFX_BODY_SHADOW] USE_PROJECTION=1` (needs Extra FX) `REALIGN_SHADOW=1`;
  `[FAKESHADOWSFX_TYRE_SHADOW] DISTANCE_FADE_END=0.2`; `[EXTRAS] DISTANT_HEADLIGHTS_GLARE2=1`,
  `OVERHEATING=1` (smoke and flames from wheels around 300 C).
- **skidmarks_fx**: `[NEW_IMPLEMENTATION] ENABLED=1 LIMIT=200 ADVANCED_BLENDING=0 SAVE_BETWEEN_SESSIONS=0`;
  `[NEW_IMPLEMENTATION_SAVING] FADE_LOADED_DAYS=0.03,1 FADE_MULT_OFFROAD=10 FADE_MULT_DIRT=10 FADE_MULT_SCRATCH=3`;
  `[STORAGE] RESET_WITH_NEW_SESSION=0 RESET_WITH_RESTART=0`; `[DEV] DUMP_SKIDMARKS_AS_OBJ=0`.
- **windscreen_fx**: `[BASIC] DISABLE_TEXTURE=0`; `[SHADOWS] ENABLE=1 BLUR_MULT=1 BLUR_MAX_SPEED=80
  BLUR_RANDOMIZE_MULT=0.8`; `[REFLECTION] ENABLED=0 INTENSITY_MULT=0.3 BLUR=0.01 ENABLED_WITH_DLSS=0`; `[TWEAKS]
  REFLECTIONS_FOR_EXTERIOR_VIEWS=1`; `[DEV] RAIN_AMOUNT=0`.
- **graphics_adjustments** (140 keys): `[RENDER_SCALE] SCALE=1 SCREENSHOTS_SCALE=1 QUALITY=5` (3 point, 0 linear,
  1 sharp bicubic, 2 Lanczos, 4 Catmull-Rom, 5 Snapdragon GSR) `SHARPEN=0.5`; `[FSR] ACTIVE=0 OLD_IMPLEMENTATION=0`
  (13 methods, see 3.8) plus per-method `QUALITY_*` / `SHARPEN_*` keys; `[ANTIALIASING] MODE=CMAA` (DISABLED,
  DEFAULT, YEBIS, CMAA, FXAA, FXAA3, SMAA) `QUALITY=HIGH`; `[LODS] CARS_DISTANCE_MULT=1 TRACK_DISTANCE_MULT=1
  TREES_DISTANCE_MULT=1 FALLBACK_LODS=1 LODLESS_LIMIT=40 LIMIT_VISIBLE_CARS=1`; `[VISIBLE_CARS]` (4 values each:
  LOD A, simplified A, simplified more, LOD D; MAIN=2,4,10,200, MIRROR=1,3,7,12); `[ORIGIN_SHIFT] ENABLED=1`;
  `[ADAPTIVE_CLIP_PLANES] ENABLED=1 NEAR_PLANE=0.1,10 FAR_PLANE=20000,30000 FOV_RANGE=0.5,12 FOV_EXP=2`;
  `[ANTIALIASING_MSAA] CUSTOM_RESOLVE=1 CUSTOM_KERNEL=0`; `[COLOR_BUFFER2] ACTIVE=1 FULL_RESOLUTION=1`;
  `[TWEAKS] TILT_MIP_BIAS=-2.5 DRIVER_TILT_GFORCES=1 CAR_BEND_SEATBELTS=1`; `[SCRIPTABLE_FILTER] ENABLED=0
  IMPLEMENTATION=tunnel`; `[SHADER_REPLACEMENTS] ALLOW_TAG_NEW_CARPAINT / _NEW_WATER / _PBR = 1`; `[HUMAN_SHADER]
  ENABLED=1`; `[NEURAL_RENDERING] ACTIVE=0`.
- **custom_rendering_modes**: `[MODE_360] FRAME_RESOLUTION=1024 FRAMES_MARGIN=0.1` (scene drawn six times);
  `[MODE_FISHEYE] FRAME_RESOLUTION=1024 MODE=1` (0 stereographic, 1 equidistant, 2 equisolid, 3 orthographic)
  `SCALE_LINEAR=0.6 SCALE_ANGLE=1` (scene drawn five times). `data_manifest.ini` lists also split screen and OpenXR.
  Needs post-processing and FXAA enabled in AC settings.
- **nice_screenshots**: `[ACCUMULATION_AA] ENABLED=1 ITERATIONS=24 RESOLUTION_MULTIPLIER=1 RANGE_MULT=1 MIP_LOD_BIAS=-2
  SHARPENING=0.05 PERCAR_CUBEMAPS=1 DISABLE_FLASHING=1`; `[BRIGHTNESS_BOOST] HDR=1` (2 accurate, 1 estimated,
  0 weighted LDR, -1 off); `[ACCUMULATION_COMPONENTS] SUN_SHADOWS=1 DYNAMIC_SHADOWS=1 CUBEMAP=1`; `[ACCUMULATION_DOF]
  ENABLED=0`; `[ACCUMULATION_BLUR] ENABLED=0 TIME_MS=16`; `[ACCUMULATION_INTERMEDIATE_SAVING] INTERVAL=30`;
  `[NAME_FORMAT] MODE=ORIGINAL`; `[WINDOWS_IMAGING_COMPONENT2] QUALITY=99`.
- **track_adjustments**: `[SEASONS] ALLOW_ADJUSTMENTS=1` (0 never with midsummer trajectory, 0.5 never with actual
  trajectory, 1 with date set, 2 always on); `[SEASONS_DEV] ALLOW_EXTRAS / ALLOW_HOLIDAYS / ALLOW_ADJUSTMENTS /
  ALLOW_DISPLAYS=1`; `[MISCELLANEOUS] SNAP_TYRE_GROOVES_TO_SURFACE=1 TREES_RECEIVE_SHADOWS=1 TREES_A2C_DITHERING=0
  TREES_LODS_DITHERING=1`; `[SPECTATORS] ADJUST_AMOUNT=1 NEW_RENDER_DYNAMIC_AMOUNT=1 NEW_RENDER_FLASHES=1`;
  `[CREW] HIDE_*`.
- **smart_mirror**: `[BASIC] ACTIVE_ON_FOCUSED=1`; `[PERFORMANCE] COMPACT_FORMAT=1 SKIP_FRAMES=0`;
  `[CUSTOM_RENDER_DISTANCE] ENABLED=0 DISTANCE=1600` (original: 800 high quality, 400 low); `[SHAKING] ENABLED=1
  BLUR=1`; `[REAL_MIRRORS] ENABLED=0 ALTER_FOV=2` (0 basic .. 3 accurate) `SHOW_CAR=1 SHOW_HEAD=1 RENDER_PER_FRAME=0`;
  `[VIRTUAL_MIRROR_YEBIS] ENABLED2=1`; `[MONITOR_SHADER] ENABLED=1`; `[VIRTUAL_MIRROR_ADAPTIVE]` with four
  `[PIECE_n]` layout rectangles written as Lua expressions of `screen.width`.
- **car_instruments**: `USE_CORRECT_SPEEDOMETERS=1`, `USE_CORRECT_TACHOMETERS=1`, `USE_NAVIGATORS=1`,
  `USE_BREAKABLE_LIGHTS=1`, `LED_PANEL_MODE=BLANCPAIN`, `[INPUT] USE_HIGHBEAM_BY_DEFAULT=1`,
  `USE_AUTOMATIC_TURNING_LIGHTS=1`, `TURNING_LIGHTS_*_THRESHOLD`; `[FUEL_LIGHT_HUD]` (Lua expressions).
- **dxgi_tweaks / small_tweaks / surfaces_fx**: not picture modules in the strict sense (swap chain, VRR, HDR output
  `[HDR] ENABLED=0`; driver, TTS, OBS, TrackIR keys; custom offroad physics). Listed for completeness only.

### 3.3 WeatherFX in depth

**Architecture** `[ini][lua][log-csp]`

1. *Launcher inputs.* Content Manager writes into `race.ini` the vanilla keys and its own, which CSP reads
   (strings in `dwrite.dll` .rdata; the owner's `race.ini` shows the values): `[LIGHTING] SUN_ANGLE` (the vanilla
   clamped value; -16.00 in the file now), `TIME_MULT`, `CLOUD_SPEED`, `__CM_UNCLAMPED_SUN_ANGLE` (the real
   angle past the vanilla +/-80 range), `__CM_DATE`, `__CM_DATE_USE_TIME`, `__CM_WEATHER_TYPE` (0..32, value 15 =
   Clear here), `__CM_WEATHER_CONTROLLER` (here `base`), `__CM_WEATHER_HUMIDITY`, `__CM_WEATHER_PRESSURE`,
   `__TRACK_GEOTAG_LAT` / `__TRACK_GEOTAG_LONG` (43.7401 / 7.4266), `__TRACK_TIMEZONE_BASE_OFFSET=3600`,
   `__TRACK_TIMEZONE_DTS=1`, `__TRACK_TIMEZONE_OFFSET=7200`; `[TEMPERATURE] AMBIENT / ROAD`; `[WEATHER] NAME`
   (a vanilla folder, here `3_clear`). `data_manifest.ini [FEATURES]` advertises the contract to launchers:
   `CONDITIONS_24H=1`, `CONDITIONS_SPECIFIC_DATE=1`, `WEATHERFX_LAUNCHER_CONTROLLED=1`, `SNOW=rain_fx.ini`.
2. *Fallback data inside the DLL.* If the launcher gives no geotag, CSP has its own table, the mirrored
   `config/data_track_params.ini`: **435 sections** (432 with `LATITUDE`, `LONGITUDE`, `TIMEZONE` as an IANA name,
   92 with `HEADING_ANGLE` = the angle of the track's north in radians, 403 with `NAME`); 24 of the 26 installed
   tracks are in it (not `ct_backwoods`, `montreal`). Time-zone rules come from an embedded `internal/tzdata`
   (IANA files + `windowszones.xml`, ~0.9 MB uncompressed). `[dll]`
3. *Controller* (`weather-controllers/<name>/controller.lua` + `manifest.ini` + `settings.ini`; Lua SDK
   `ac_wfx_controller`, 919 functions of which 124 are its own). It builds an `ac.ConditionsSet`: `currentType`,
   `upcomingType`, `transition` (0..1), `variableA/B/C` (free data between controller and style), `temperatures`
   (ambient, road), `wind` (direction, speedFrom, speedTo), `humidity`, `pressure` (Pa), `trackState` (sessionStart,
   sessionTransfer, randomness, lapGain), `rainIntensity`, `rainWetness`, `rainWater`; it publishes with
   `ac.setConditionsSet2()`. `[sdk]`
4. *Style / implementation* (`weather/<name>/weather.lua` + `manifest.ini` + `settings.ini`; SDK `ac_wfx_impl`,
   2,082 functions, 1,287 own). Reads the set, converts it, and applies it. The shipped `base` style is
   57 files: `weather.lua`, 24 `src/*.lua` (conditions converter, sun / sky / light / ambient / fog application,
   cloud generation in cells, light pollution, aurora, rain haze, volumetric fog, lightning, eclipse glare, meteors,
   a lightweight post-processing replacement, linear-colour-space stub, audio, showroom modes), 27 shader files
   (`.fx`, `.hlsl`: aurora, clouds, fog, lightning, post-processing, rain haze, volumetric fog), `clouds/atlas.dds`
   (2.8 MB) and `clouds/noise3D.dds` (1.0 MB), `res/dirt.jpg`.
5. *Style settings* (`weather/base/settings.ini`, user copy in `Documents/.../cfg/extension/state/lua/wfx_impl/
   base.ini`): `[LINEAR_COLOR_SPACE] ENABLED=1 DIM_EMISSIVES=1 BLUE_NIGHTS=0`, `[POSTPROCESSING]
   LIGHTWEIGHT_REPLACEMENT=0 FILM_GRAIN LENS_DIRT GLARE_CHROMATIC_ABERRATION VIRTUAL_MIRROR_GLARE`,
   `[EXTRA_EFFECTS] UPPER_CLOUDS=1 AURORAS=1 FOG_VOLUMETRIC=0 FOG_ABOVE=1 RAIN_HAZE=1 LIGHTNING=1`. The style
   forces linear colour space OFF when post-processing is off (`ac.getSim().isPostProcessingActive`): this is the
   owner's case.
6. *C++ renderer behind the API.* Sky V2 pixel shaders (`wfxSkyV2_ps`, `_quality`, `_reflections`, cover
   variants), clouds (`wfxClouds*`, `wfxCloudsV2*`, a `Q` quality variant, `wfxCloudsCover*`), noise generation
   (`wfxClouds_noise*`), earth for space views (`wfxEarth*`), cloud-shadow blur, Gaussian splats
   (`dclGausplats*`, 27 shaders, "switching to default Weather FX style" if the style is not splat-aware).
   Strings at 0x22a5c50..0x22a6230. `[dll]`
7. *Online.* Conditions come from the server's conditions plugin; the dll has format names `WEATHERFX_V1/V2/V3`
   (0x2225758 / 0x2225748 / 0x2225738) [interp: protocol versions]. `ac.getSim().timeMultiplier` doc: "online,
   it's either 0 with basic conditions, averaged with WeatherFX conditions...".

**Types.** `ac.WeatherType` has 33 values: 0..2 thunderstorms (light, normal, heavy), 3..5 drizzle, 6..8 rain,
9..11 snow, 12..14 sleet, 15 Clear, 16 Few clouds, 17 Scattered, 18 Broken, 19 Overcast, 20 Fog, 21 Mist, 22 Smoke,
23 Haze, 24 Sand, 25 Dust, 26 Squalls, 27 Tornado, 28 Hurricane, 29 Cold, 30 Hot, 31 Windy, 32 Hail; the base style
adds a private 100 `NoClouds` ("used by Sol"). The style turns each type into 9 numbers: `fog`, `clear`, `clouds`,
`cloudsDensity`, `tint` (rgb), `saturation`, `thunder`, `pollution`, `snow` (36 definitions in
`conditions_converter.lua`). `[lua]` AC's own `content/weather/` still has 7 folders (`1_heavy_fog` .. `7_heavy_clouds`)
and is what vanilla and rustyAC use. The "weather types folder" of the task is therefore *not a folder of types*:
a type is an enum value plus the table above; a **style** is a folder under `extension/weather`.

**Controllers.**

| Controller | Does | Settings |
|---|---|---|
| `base` (1.9 KB) | static: copies the launcher's type, temperatures, wind, humidity, pressure and track state; sets rain from the type; guesses humidity / pressure if missing | none |
| `base-live` (7.5 KB + 5 service files, 4 online providers) | real weather from OpenWeather, Weatherbit.io, Open-Meteo or WeatherAPI.com (+ an example accessor); `FOLLOWS_SELECTED_TEMPERATURE = not SETTINGS/REAL_TEMPERATURE` | `PROVIDER`, `REAL_TEMPERATURE`, `REAL_WIND`, `REAL_RAIN` |
| `base-schedule` (4.8 KB; new in preview649, absent in 0.2.11 / preview212) | 1..5 slots (`SLOT_n` = a type id), `TIME_PER_SLOT` 1..30 min, `RANDOM`; type transition in the last half of the slot (max 120 s), rain ramps in the last 20 s, a wetness film grows 0.005/s and dries 0.0002/s, puddles follow the film | `SLOTS`, `SLOT_0..4`, `RANDOM`, `TIME_PER_SLOT` |

`weather_fx.ini` defaults to `CONTROLLER=sol`, but no `sol` folder ships here; Content Manager's
`__CM_WEATHER_CONTROLLER` overrides it (the log on this PC: "Weather FX controller loaded"). [log-csp][interp]

**What the style sets (the output vocabulary; ~250 `ac.set*` calls).** The base style, per update (every 3rd
frame, or when the sun moved by more than about 0.003 rad (dot product of the old and new direction below 0.999995)), calls: sun and moon direction reads; sky V2 model parameters
(Rayleigh and Mie coefficients, turbidity, zenith lengths, primaries, refractive index, number of molecules,
luminance, gamma, saturation, sun shape, y offset and scale for the Earth's curvature, rainbow);
`ac.addSkyExtraGradient` (max 16 gradients: general multiplier, cover, horizon, city haze, eclipse, thunder);
`ac.setLightDirection / setLightColor / setSpecularColor / setSunSpecularMultiplier`; `ac.setCloudsLight`;
`ac.setAmbientColor / setExtraAmbientColor + Direction / setDistantAmbientColor / setBaseAmbientColor`;
`ac.setFogColor / Distance / Exponent / Blend / Atmosphere / Height / Density`, `ac.setNearbyFog`,
`ac.setHorizonFogMultiplier`, `ac.setSkyFogMultiplier`, `ac.setFogBacklit*`; `ac.setSkyMoon* / Stars* / Planets*`;
`ac.setShadows(On|Off)` and `ac.setShadowsResolution` (shadows are switched off when the light colour value is below
1e-5 and blurred in fog); `ac.setBrightnessMult`, `ac.setWhiteReferencePoint`, `ac.setHDRToLDRConversionHints`,
`ac.setWeatherLightsMultiplier(2)`, `ac.setEmissiveMultiplier`, `ac.setTrueEmissiveMultiplier`, `ac.setGlowBrightness`,
`ac.setFresnelGamma`, `ac.setReflectionEmissiveBoost`, `ac.setLambertGamma`; `ac.adjustTrackVAO`,
`ac.adjustDynamicAOSamples`; `ac.setWeatherFakeShadowOpacity / Concentration`; `ac.setAiHeadlightsSuggestion`;
`ac.setTrackHeatFactor`, `ac.setSnowMix`, `ac.configureGrassShading`; god rays (`ac.setGodrays*`); the cloud system
(`ac.SkyCloudV2`, `ac.SkyCloudMaterial` with 20 fields, `ac.generateCloudMap`, `ac.setCloudShadow*`). `[lua][sdk]`

**Time and seasons.** `ac.getSim()` has `timestamp` (local seconds since 1970), `timeTotalSeconds`, `dayOfYear`,
`timeMultiplier`, `neutralSunTrajectoryTimestamp`, `timePostOffset`, `isTimeOffsetIgnored`. Debug / app calls:
`ac.setWeatherTimeOffset(seconds, instant)` ("offset above 24 hours counts as race date being set ... an equinox
trajectory ... will use real trajectory now"), `ac.setWeatherExactUTC0Timestamp`, `ac.setWeatherTimeMultiplier`,
`ac.overrideReplayConditions`. Three sun trajectories exist: midsummer, equinox ("neutral"), and the real one for
the date (`track_adjustments [SEASONS] ALLOW_ADJUSTMENTS` 0 / 0.5 / 1 / 2). Seasons reach the content as the input
`YEAR_PROGRESS` and `SEASON_*_NORTH` conditions (5 LUTs in `tracks/common/conditions.ini`: summer, summer 2, mud,
winter, autumn). The log of the last session: "track timezone=Europe/Berlin, base offset=1, DST offset=1; start
time=84036 (2026-10-12 23:20:36)": a night start on today's date, which vanilla cannot do. `[log-csp]`

### 3.4 True night, moonlight, wipers, reverse lights: what exists and what rustyAC would need

**Vanilla (what rustyAC already ports 1:1, `crates/rustyac-render/src/sky.rs` and `lighting.rs`).**
`SunAnimator::update` (0x1401adc10) advances `lighting.angle = base + t * k * velocity` and **clamps it to
[-80, +80]** every frame; the light direction is the vector (0,-1,0) rotated by the track's pitch
(`SUN_PITCH_ANGLE`), the angle and the track heading; the sun, ambient, horizon and sky colours are
`lerp(low, high, (1 - clamp(-light.y, 0, 1)) ^ ANGLE_GAMMA)` of the 5 pairs in `colorCurves.ini`. So the
darkest light vanilla reaches in a running session is the blend at angle 80 (dusk); the darkest it can express at
all is the "low" colour. The sky is a generated dome with no stars or moon. At angle 80 the sun is still about 10
degrees above the horizon (for zero pitch and heading the light y is -cos(angle), so t = 0.17); the "low" colour
itself is reached at 90 and beyond. The proof frames of `docs/port` use SUN_ANGLE 84 / 86 where no running
`SunAnimator` clamps them [interp]. `[prior][dll]`

**CSP's route** `[lua][sdk][dll]`

1. *Time model.* A real clock + date + latitude / longitude / time zone, giving the sun and moon directions by
   astronomy in C++ (`ac.getSunDirectionTo`, `getMoonDirectionTo`, `getMoonFraction` (phase), `getSunMoonAngle`
   (eclipses; pi/2 when real trajectory is off), `getDayOfTheYear`, `getTrackCoordinates`). The algorithm itself is
   in the dll and was not read (open question). The "SUN" input of track conditions is the sun angle in degrees,
   0 noon .. 180; track light series switch at **88 degrees** (`LUT=(|0=0|88=0|88=1|180=1)`, `NIGHT_SMOOTH`
   `LAG=0.97`, `NIGHT_SLOW_HEATING` with `LAG_DELAY_ON=1800` s).
2. *Light.* In the style: below the horizon the sun is dimmed to zero between light-direction y of -0.03 and 0 and a
   "below horizon correction" lifts the direction to y=0.02 for the last sliver. When the night factor `realNightK`
   (sun y from +0.05 to -0.2) is above 0.7 and the moon is above 0 .. 0.12 elevation, the **moon becomes the scene
   light**: direction = moon direction, colour = sky absorption at the moon direction x `MoonColor` x
   `MoonLightMult` x light-pollution factor x moon phase fraction x moon eclipse factor x `clear`. Constants:
   linear path `MoonLightMult=0.005`, `MoonColor=(1,1.5,2)`; legacy path (post-processing off) `MoonLightMult=0.5`,
   `MoonColor=(0.6,1.2,2)x1.5`. Clouds get their own light so they stay lit below the horizon
   (`cloudSunLight` from sun y -0.23 to -0.115).
3. *Ambient and sky at night.* Legacy path: ambient = sky colour at zenith x `AmbientLightIntensity=10` with
   saturation 0.25..0.5; a sky "general multiplier" gradient drives the sky towards deep blue
   (`(0.2, 0.8, 1.6)` blend at `NightK^2`); sky luminance falls with `(1-NightK)^4`; stars, planets and the moon
   texture (`textures/weather_fx/{starmap 22 MB, moon 1.4 MB, earth 2.8 MB}.dds`) fade in with `NightK`, reduced by
   light pollution (`ac.getTrackLightPollution`: position, radius, tint, density from a `[LIGHT_POLLUTION]` section
   of the track config, 3 tracks here). The linear path does the same with physically scaled numbers.
4. *Exposure (what makes it look dark yet usable).* A "fake exposure": scene brightness is estimated from ambient
   and light luminance and the camera's occlusion (tunnels); `ac.setBrightnessMult(sceneBrightness x 2)` without
   post-processing (x 3 with it), `ac.setWeatherLightsMultiplier` and `ac.setEmissiveMultiplier` raise lights and
   emissives as the scene darkens, `AdaptationSpeed=10`. With post-processing the real auto-exposure is used.
5. *Switching lights on.* Track emissives and light series use the `SUN` condition; AI headlights through
   `ac.setAiHeadlightsSuggestion` (dark ambient, fog, rain), `[MISCELLANEOUS] FORCE_HEADLIGHTS /
   SWITCH_HEADLIGHTS_WITH_AI`; cars get a real light from `LIGHT_HEADLIGHT_n` (Lighting FX).

**What rustyAC would need** (smallest useful set first) `[interp]`

- *Night-lite (no Lua, no Lighting FX):* (a) remove the +/-80 clamp in the `SunAnimator` port and let the angle run to
  180 (CM's `__CM_UNCLAMPED_SUN_ANGLE`) or take a date + time + geotag; (b) a new colour table for the sun below the
  horizon (vanilla's "low" colours stop at dusk: need ambient, sky, horizon, fog and sun colours that go close to
  black, plus a bluish moon light from the moon direction and phase); (c) a post-pass on `cbLighting` after
  `updateLightingSetttings` (the same place CSP edits: gm+0x470 buffer, flag gm+0x478) so the vanilla path stays
  the default; (d) a moon / stars layer: needs a new sky shader (vanilla has none), `starmap.dds` (22 MB inside
  `dwrite.dll`, or an own star map), `moon.dds`; (e) switch the emissive "lights on" state by the sun angle so the
  headlights and the track's emissive night materials come on (vanilla already switches headlights from the
  driver's key, track lights are not conditional at all); (f) a brightness multiplier ("fake exposure") so a
  dark scene is playable. Size: **M**, mostly the sky and the colour model; no oracle exists for the numbers
  (CSP is the reference, the style Lua is public).
- *Faithful night:* the rest of WeatherFX (3.3): sky V2 model, clouds V2 with noise generation, cloud shadows,
  fog model, light pollution, 24-hour time: **XL** (section 7 table).
- *Reverse lights:* config side only. A car's `ext_config.ini` uses the `ReverseLights` mixin of
  `cars/common/custom_emissive.ini`, which generates `[EMISSIVE_REVERSE_n]` (`NAME` meshes, `COLOR`, `CHANNEL`,
  `LAG=0.5`, `LOCATION=REAR`, `CAST_LIGHT`) and `[LIGHT_REVERSE_0]` (a real light). The trigger is the condition
  input `REVERSE` / `REVERSE_LIGHTS` (gear R), with the string `REVERSE_FADE_TIME` in the dll. Needs: the INIpp
  mixin / generator language (or hand-expanded sections), an emissive-channel switch by input, a lag filter.
  Size **S-M** once the config machinery exists; the visible change is an emissive material, a dynamic light
  needs Lighting FX. Used in the owner's installed `bmw_m3_e30` and `lotus_exige_240` configs (found by search).
- *Wipers:* (i) animation: `[ANIMATED_WIPER_n]` (`MESHES`, `UNDERLYING_SURFACE`, `GROUP_n` = mesh and sub-mesh
  selectors, `GROUP_n_AXIS`, `_PIVOT`, `_END`, `_ARM`, `_ANGLE_TO`, `_RUBBER_FROM/TO/OFFSET_*`, `_BENDING_*`,
  `USE_MESH_FOR_TRACES`) rotates mesh groups about an axis by `wiperProgress`; CSP keeps `ac.StateCar`
  `wiperModes`, `wiperMode`, `wiperSelectedMode`, `wiperSpeed`, `wiperProgress`; default is a `ksanim` wiper
  animation unless `DISABLE_WIPER_KSANIM`; (ii) control: `__EXT_WIPERS_OFF/1/2/3/4/MORE/LESS` bindings and
  automatic switching with rain (`SWITCH_WIPERS_WITH_AI=1`); (iii) water on the glass: the RainFX windscreen
  simulation (`accRainWindscreen_*`: move, narrowWipers, nmgen, blurTraces, refractionMask, finalize; wiper traces
  via `RainFX wiper traces` and `WIPER_TRACE_CAMERA_DIR`), (iv) sound: the audio channel `wipers` (manifest
  `[AUDIO_LEVELS] WIPERS = 0.8`) and `extension/sfx/csp_extras_rain.bank`. For rustyAC: (i) + (ii) are **S-M**
  and visible without any rain; (iii) is part of RainFX (**XL**); (iv) needs the CSP sound bank (FMOD, see audio).

### 3.5 Lighting: lights, light series, shadows, bounce, adaptive emissive

All of this lives in per-car and per-track config sections `[ini][lua]`:

- **Sections (counts in the installed configs, 11 track + 8 car CM-downloaded files plus the `common/` libraries):**
  `LIGHT_SERIES_n` 248 (11 files), `LIGHT_n` 57 (7), `LIGHT_EXTRA_n` 13 (3), `EMISSIVE_*` (generated), `CONDITION_n` 139 (15),
  `LIGHT_POLLUTION` 3, `BOUNCED_LIGHT` 9, `BOUNCED_INTERIOR_LIGHT` 3, `LIGHTING` 10, `FAKE_SHADOW` 38 (2), `EXTRA_FX` 5,
  `EXTRA_FX_EMISSIVE` 5.
- **`[LIGHT_n]`** keys: `COLOR` (r,g,b,brightness), `POSITION`, `DIRECTION`, `SPOT` (degrees; 0 = point light),
  `SPOT_SHARPNESS`, `RANGE`, `RANGE_GRADIENT_OFFSET`, `FADE_AT`, `FADE_SMOOTH`, `LINE_FROM` / `LINE_TO` (a line
  light), `COLOR_FROM/TO`, `SPECULAR_MULT`, `DIFFUSE_CONCENTRATION`, `SINGLE_FREQUENCY`, `CONDITION`,
  `VISIBILITY_LEVEL`, `SHADOWS`, `SHADOWS_BOOST`, `SHADOWS_EXP_FACTOR`, `SHADOWS_HALF_RESOLUTION`,
  `SHADOWS_EXTRA_BLUR`, `VOLUMETRIC_LIGHT`. Lua twin `ac.LightSource` (3 methods, 32 fields: `lightType`, `position`,
  `color`, `range`, `fadeAt`, `spot`, `linePos`, `lineColor`, `shadows`, `shadowsStatic`, `shadowsSpot`,
  `shadowsRange`, `shadowsClipPlane`, `shadowsClipSphere`, `skipLightMap`, `affectsCars`, `showInReflections`,
  `longSpecular`, `volumetricLight`).
- **`[LIGHT_SERIES_n]`**: the same light parameters applied to every mesh (or cluster of mesh vertices) matched by
  `MESHES` / `MATERIALS`: `CLUSTER_THRESHOLD` (merge close points; the dll warns "Consider decreasing
  CLUSTER_THRESHOLD or fix the mesh"), `OFFSET`, `DIRECTION`, `DIRECTION_OFFSET`, `DIRECTION_ALTER`, `COLOR_OFF`,
  `CONDITION`, `SHOW_IN_REFLECTIONS`, `LONG_SPECULAR`, `SHADOWS`, `SHADOWS_SPOT`. 36 in `spa.ini` alone; the install
  has track configs for 11 tracks.
- **Car lights.** Built-ins: `[LIGHT_HEADLIGHTS]` / `[LIGHT_BRAKES]` (set `COLOR=0,0,0,0` to turn off),
  `[LIGHT_HEADLIGHTS] POPUP_FILE=NONE` (no pop-up animation), `LIGHT_HEADLIGHT_n`, `LIGHT_REVERSE_n`,
  `LIGHT_TURNSIGNAL_LEFT/RIGHT_n` (`PREFER_FRONT`, `EMISSIVE_INDEX`), `LIGHT_EXTRA_n` (20 slots A..T bound to the
  hotkeys `__EXT_LIGHT_A..T`; `BOUND_TO`, `BIND_TO_HEADLIGHTS`, `INTERIOR_ONLY`, `EXTERIOR_ONLY`, `MIRROR`,
  `AFFECTS_TRACK`, `SELF_LIGHTNING`, `SPOT_EDGE`), `EMISSIVE_HEADLIGHTS_n` / `EMISSIVE_BRAKE_n` /
  `EMISSIVE_REVERSE_n` / `EMISSIVE_LIGHT_n` (parking) / `EMISSIVE_TURNSIGNAL_LEFT|RIGHT_n` with `NAME`, `COLOR`,
  `CHANNEL` (ksEmissive..ksEmissive10), `LAG`, `SIMULATE_HEATING` (filament warm-up), `LOCATION`, `CAST_LIGHT`,
  `ACT_AS_HEADLIGHTS`, `FALLBACK_HEADLIGHTS_COLOR`. `[LIGHTING]` per car/track: `SPECULAR_MULT`, `LIT_MULT`,
  `CAR_LIGHTS_LIT_MULT`, `EMISSIVE_HEADLIGHTS_MULT`, `INTERIOR_FAKE_SHADOW_*`. 20 extra lights, 2 mirrored
  headlights collapsing beyond 120..150 m (`NOMIRROR_*`).
- **`[CONDITION_n]`**: `NAME`, `INPUT`, `LUT` (a curve string `(|x=y|x=y|)` or a `.lut` file such as
  `flag_colors.lut`), `LAG`, `LAG_DELAY_ON/OFF/FUNC` (LINEAR, SQR, SQRT, POW3), `FLASHING_FREQUENCY / SMOOTHNESS /
  SYNCED / MIN_VALUE / NOISE_*`, `DELAY`, `SIMULATE_HEATING`, `INPUT_STAY_FOR`, `INPUT_CHANGE_DELAY`. Inputs named
  in the shipped track library: `ONE`, `NONE`, `TIME`, `AMBIENT`, `FOG`, `SUN`, `RACING_FLAG`/`FLAG_TYPE`,
  `YEAR_PROGRESS`; the dll adds a table of ~165 car and session inputs (`SPEED_KMH`, `BRAKE`, `STEER`, `HANDBRAKE`,
  `CLUTCH`, `HEADLIGHTS`, `LOWBEAM`, `HIGHBEAM`, `BRAKE_LIGHTS`, `REVERSE`, `HAZARD`, `TURNSIGNAL*`,
  `CORNERINGLAMP_*`, `OPENDOORS`, `SEATBELT`, `EXTRA_A..T`, `WIPERS_MODE`, `WIPERS_PROGRESS`, `GEAR_*`, tyre,
  fuel, KERS, DRS, lap and wind values, `TIME_HOURS`, `TIME_MINUTES`). `[dll]` (strings 0x2218xxx..0x2275xxx)
- **Shadows from lights.** Exponential shadow maps: `shadowsBoost`, `shadowsExponentialFactor`; the module default is
  `[SHADOWS] ENABLED=0`; cars cast into track-light shadows (`CARS_IN_TRACK_SHADOWS=1`). Shaders `accShadowDynamic_*`
  (alphatest, attributeless, biased, car, collider, flag, headlightsCollider, main, skinned, spectators).
- **Headlight projection.** `[REFRACTING_HEADLIGHT_0]` per car (refraction of bulbs; shaders `accRefractionBulbsDetect`,
  `accRefractionExtendDepth`, `accRefractionMergeWithMask`) and the "headlight texture" consideration of `LIGHT_EXTRA`
  (`CONSIDER_HEADLIGHTS_TEXTURE`, "doing nothing atm").
- **Bounced light.** Lighting FX `BOUNCED_LIGHT_MULT=0.35` etc. and `ac.setWeatherBouncedLightMultiplier /
  BouncedInteriorLightMultiplier / BouncedLightSh*`; a top-down "light map" of the track (`accTrackLightMap*` 8
  shaders, `accCarLightMap*` 3, `accLightMap{Mesh,Rect}Occluder`) that Extra FX `[LIGHT_BOUNCE]` reads; per-track
  `[BOUNCED_LIGHT] SUPPORTED, OBJECT_MATERIALS, SURFACE_MATERIALS, OCCLUDING_MATERIALS, CLIP_POINT_n, BASE_CLIP_Y,
  CAUSTICS` (9 tracks here).
- **Adaptive emissive / white reference.** `ac.setAdaptiveEmissiveMultiplier(nearby, distant)`, material flags
  `ADAPTIVE_EMISSIVE`, `ADAPTIVE_NEARBY_EMISSIVE`, `ADAPTIVE_DISTANT_EMISSIVE`, `WHITE_REFERENCE_POINT_EMISSIVE`
  (the generic track config binds `PIT_indicator` to it), `ac.setWhiteReferencePoint` (default 3; UI and name tags
  use it), `weather_fx [TWEAKS] APPLY_AUTOMATIC_WHITE_REF_POINT_FIX`.
- **Built-in editor.** The CSP Debug app and the in-game Lights UI create point / spot / line lights, copy them
  to the clipboard as config text, and show "Light map", "Lights (visible)", "Max lights count", "Log lights"
  (strings 0x2235090..0x2235df0). `[dll]`

### 3.6 Materials, shader replacement, model replacement, mesh adjustment, VAO patches

- **`[SHADER_REPLACEMENT_n]`** (158 in the installed configs; the generic car config uses 4 of them for drivers,
  alpha fixes and double-sided shadows). Filters: `MATERIALS`, `MESHES` (a small expression language:
  `{ driverPiece:yes & alphaBlend:yes }`, `shader:ksPerPixel`, `insideInterior:yes`, `isTextureDefault:...`,
  `emissive:no`, `castsShadows:y`, `! ( ... )`, `?` wildcard, `|`). Actions: `SHADER=<name>`, `PROP_n = name, v...`
  (shader variable; `PROP_...` = auto-index), `RESOURCE_n` / `RESOURCE_TEXTURE_n` / `RESOURCE_FILE_n` /
  `RESOURCE_COLOR_n`, `BLEND_MODE` (OPAQUE, ALPHA_TEST, ALPHA_BLEND, MULTIPLY), `CULL_MODE` (NONE, DOUBLESIDED),
  `DEPTH_MODE` (NORMAL_FORCED, NOWRITE, OFF, DEPTH_NORMAL), `IS_TRANSPARENT`, `CAST_SHADOWS`,
  `DOUBLE_FACE_SHADOW_BIASED`, `SEMITRANSPARENT_SHADOWS`, `LOD_IN` / `LOD_OUT`, `MOVE_MESH_BEHIND`,
  `EXTRA_MASK_PASS(_OPACITY/_COLOR)`, `LINEAR_ALPHA_FIX`, `DISTANT_SHADOWS`, `CUBEMAPLESS_REFLECTION`,
  `FILL_MISSING_TEXTURES`, `TAGS`, `MATERIAL_FLAG_n`, `SHARED_MATERIALS`, `ADAPTIVE_EMISSIVE`. In Lua:
  `ac.SceneReference:applyShaderReplacements()`, `dumpShaderReplacements()`.
- **Replacement shader names used in the shipped configs** (38 distinct): Kunos `ks*` ones
  (`ksPerPixelMultiMap_emissive` 17x, `ksPerPixelAlpha`, `ksTree`, `ksWindscreen`, `ksPerPixel_tilingfix`,
  `ksPerPixel_ppfog`, ...), CSP `ne*` (`nePBR_MultiMap`, `nePBR_MultiMap_NMDetail(_ClearCoat)`,
  `nePerPixel*_heating`, `nePerPixelMultiMap_windows`, `nePerPixel_light`) and `sm*` (`smCarPaint` + `_old`,
  `_rainbow`, `_rainbowChrome`, `_chameleon`, `_chromaFlair`; `smGlass`, `smGlass_emissive`, `smGlass_phel`,
  `smWaterSurface`, `smWaterDrain`, `smDigitalScreen(GlassOverlay)`, `smLicensePlate`). Meaning of the prefixes is
  not documented [interp: "new engine", "smart"]. The DLL names ~620 compiled programs by `acc*` / `wfx*` /
  `dcl*` / `flg*` / `pfx*` / `ins*` families (694 `.fxo` strings; 6,683 DXBC blobs, 123.2 MB uncompressed,
  30.9 MB compressed, median 11.9 KB, largest 650 KB, archive 1 of the overlay).
- **Libraries (`extension/config/cars/common`)**: `materials_carpaint.ini` 355 lines: templates `Material_CarPaint_Solid`,
  `_Metallic`, `_Chrome`, `_Pearl`, `_Matte`, `_Old`, `_Chameleon`, `_Metal`, `_ChromaFlair`, `_Rainbow`,
  `_RainbowChrome`, with `FresnelMax`, `FresnelC`, `AmbientSpecular`, `ClearCoatThickness`, `FlakesK`,
  `ColoredSpecular`, `PearlescentSpecular`, `ChameleonColorA/B`, `SpecularBase`, `NormalsNoise`; `materials_glass.ini` 149
  (`Material_Glass`, `GlassSide`, `MultiEmissiveGlass`, `PhotoelasticGlass`); `materials_interior.ini` 449;
  `materials_pbr.ini` 123 (`Material_PBR`: `TextureAlbedo`, `TextureNormal`, `TextureEmAoMeRo` (emissive / AO /
  metalness / roughness in RGBA), `TextureSpecular`, `TextureVarianceAO`, `F0=0.04`, parallax keys
  `DisplacementMax/Min`, `ParallaxScale`, `ParallaxSteps=20`); `materials_license_plate.ini` 165 (also
  `data_*` for plates); `materials_toon.ini`; `custom_emissive.ini`; `displays.ini`; `navigators*.ini`;
  `tyre_patterns.ini`; `f1_*.ini`, `gt3_*.ini`; `human_surface_template.ini`. Track side `materials_track.ini`,
  `materials_pbr.ini`, `materials_glass.ini`, `particles_track.ini`, `custom_emissive.ini`. PBR textures
  `pbr_{carpet,fabric,leather_*,metal,plastic}.dds` (8 x 1.4 MB), `dfg.dds`, `water_normal.dds` (5.6 MB) are in the
  mirrored `textures/common`.
- **`[MATERIAL_ADJUSTMENT_n]`** (331 sections in 12 files): `MATERIALS` / `MESHES`, `KEY_n` + `VALUE_n`
  (set shader variable n), `VALUE_n_OFF` / `OFF_VALUE_n` (value when the condition is off), `CONDITION`,
  `VISIBILITY_LEVEL`, `FADE_AT`, `BLEND_MODE`, `COLOR`, `INPUT_STAY_FOR`, `SINGLE_FREQUENCY`, light params when
  the adjustment spawns a light. Typical use: gate `ksEmissive` by `NIGHT_SMOOTH` or `HEADLIGHTS`; zero a wrong
  emissive (`VALUE_0=0`).
- **`[MODEL_REPLACEMENT_n]`** (26): `FILE`, `INSERT` (an extra `.kn5` merged into the model; the installed tracks
  have 23 such helper `.kn5` files such as `spa__lights.kn5`, `*Glass*.kn5`), `INSERT_AFTER` (node), `HIDE`.
  **`[MESH_ADJUSTMENT_n]`** (4): `MESHES`, `IS_TRANSPARENT`, `MOVE_TO`. Others: `[SMOOTH_NORMALS]`
  (`PRECISION`, `NORMAL_CONTRIBUTION`, `NORMAL_REPAIR`), `[TRACK_OCCLUDER_WALL_n]` / `[TRACK_OCCLUDER_BOX_n]`
  (`POINT_n`, used by rain occlusion and light / audio occlusion), `[FAKE_SHADOW_n]` (`POSITION_n` x4, `OPACITY`,
  `SQUAREDNESS`, `LINKED_TO`, `CORNER_MODIFIERS`), `[DISPLAY_n]` track displays (`CAMERA_n`, `CAMERA_n_TARGET`,
  `_FOV`, `REGION_*`, `OUTPUT_*`), `[ANIMATED_n]` (spinning windmills: `NODE`, `SPIN_AXIS`, `WINDMILL_DIR`,
  `SPEED_MULT`, `LAG_UP/DOWN`), `[WIND] DYNAMIC_FLAGS=AUTO`, `[LOCAL_CUBEMAP_n]` (`MESHES`, `POSITION`, `OFFSET`,
  `HIGH_RESOLUTION`, `IS_DYNAMIC`, `STATIC_FACES`, `EXCLUDE_NODES`), `[TRACK_MAP]`.
- **INIpp** (the dll function `parseINIppFile` at string 0x22a85e8; also "ConfigsCache" binary cache in
  `cache/csp_configs`): `[INCLUDE]` / `[INCLUDE: file]` with `ACTIVE = $"<lua expression>"`; `[DEFAULTS]`
  (named values `$Name`); `[FUNCTION: Name]` with `ARGUMENTS` and `CODE` (Lua) and `[USE: functions_base.lua]`;
  `[TEMPLATE: Name EXTENDS Base]` with `@OUTPUT`, `@GENERATOR = Template, args`, `@MIXIN`, `@ACTIVE`,
  `@GENERATOR_STARTING_INDEX`; `[MIXIN: Name]`; `${Name:type:default}` and `$Name` substitution; numbered
  sections with `_...` auto-index (`SHADER_REPLACEMENT_...`); inside expressions `read('car/id')`,
  `read('settings/FIXES/..')`, `get('BASIC','KEY')`, `has('SECTION')`, `def(a, b)`, `discard()`. Lists of other
  files via `CAR = a_?` and `TRACK = $KunosTracks` filters in `data_gbuffer.ini` (23 `[FIX_...]` entries: `SKIP_GBUFFER`,
  `MASK_GBUFFER`, `DELAYED_RENDER`, `GLASS_FILTER`, `ALPHA_TEST_GBUFFER(_THRESHOLD)`, `AUTOFIX=0`).
  Priority: `extension/config/{cars,tracks}/generic.ini` (every car / track), `loaded/<id>.ini` (CM downloads from the
  public `acc-extension-config` repository; the header of `spa.ini` says so), the content's own
  `extension/ext_config.ini`. `[ini][lua]`
- **VAO patches** (`extension/vao-patches/<track>.vao-patch`, `vao-patches-cars/<car>.vao-patch`). A `.vao-patch`
  is a **plain ZIP** (checked: `PK`, deflate, not encrypted): `Patch_v4.data` or `Patch_v5.data` (older: `Patch.data`
  + `Manifest.json`, 91 bytes), optional `ExtraSamples.data` (170..288 KB: "extra VAO samples", second version,
  used for cars and dynamic objects so they darken in tunnels), `Config.ini`, and for one track `Baked Shadows
  Params.txt`. Config sections: `[LIGHTING] OPACITY, BRIGHTNESS` (0.85 / 1.1 for Nordschleife and Monaco, 0.9 / 1.2
  for Monaco 2017), `[SPLIT_AO] COCKPIT_HR, DOOR_NODES, DOOR_EXP, HEADLIGHTS_NODES, STEERING_WHEEL_NODES,
  WING_ANIM_n_*` (separate AO for animated parts), car patches also `[SHADER_REPLACEMENT_0_FIX_INT / _FIX_DRIVER]
  PROP_0=ksAmbient,*2.0`. The log shows how it is applied: per `KN5` node, mesh count and time ("monaco_2017.kn5: 1125
  meshes, 249 ms"). So it is **baked per-vertex ambient occlusion data keyed to mesh order**, applied to vertex
  attributes while the model loads (hook `TrackAvatar::init3D` 0x11258b0 and `KN5IO`); the dll can also bake
  ("GTOCPU: update raw VAO", "VAO OptiX bakery" for authors). Counts here: 10 track patches (29.8 MB; 9 of the 26
  installed tracks), 7 car patches (1.4 MB). Lua: `ac.setVAOMode`, `ac.VAODebugMode`, `ac.isVertexAoPatchApplied`,
  `ac.adjustTrackVAO`, `ac.setVAOExponent`, `ac.getVAOMinMax`. The binary layout of `Patch_v*.data` was NOT decoded. `[dll]`

### 3.7 Shadows, reflections, RainFX, GrassFX, particles, tyres, skid marks

- **Shadows.** Smart Shadows (3.2) hooks the three-cascade code; vanilla has 3 cascades from `SHADOW_MAP_SIZE`
  and the port reproduces them. CSP adds: automatic splits with distance and lambda, a 4th cascade to 1,500 m
  ("also adds shadows in mirrors and reflections"), view-independent stabilised matrices (`LOOKLESS`), smooth
  cascade overlap, lazier updates. Shader family `accShadowGen_*` 19 variants (alpha test, attributeless, skinned,
  tree, flags, half opacity, custom opacity, ghost, collider, and `_cfx` copies [interp: the coloured-shadow ones]). `colorful_shadowing` is a per-material
  feature: `COLORFUL_SHADOWING`, `_COLOR`, `_COLOR_MULT`, `_OPACITY_CLIP` (strings 0x2252d50..0x2252f08) [interp:
  coloured light through stained glass]. Fake shadows: `[FAKESHADOWSFX_*]` plus `ac.setWeatherFakeShadow*`; shaders
  `accFakeCarShadows_*`, `accTrackFakeShadow*`, `accCarFakeShadow{Gen,Final,Wheel}`.
- **Reflections.** Vanilla: a static cube map from `cubemap_model.kn5` plus a dynamic cube map updated
  `FACES_PER_FRAME`. CSP: partial reprojection, 512 default, physically-based prefiltered sampling (cbuffer
  `ac_ext::data_ac_ext_reflections_fx::cbuffer_ibl`), interior mask, "local cubemaps" for cars and tracks, WeatherFX
  makes the static cube map unnecessary (sky shader `wfxSkyV2_ps_reflections`), `ac.setReflections*`,
  `ac.pauseCubemapUpdates` (frozen during a thunder flash), `ac.adjustCubemapReprojectionBrightness`.
- **RainFX.** Visual: 61 shader programs `accRain*` (+ 7 `accSnow*`): drops (compute generation + count), hits,
  splashes, streams (compute; gbuffer and reflection variants), puddle generation / mesh / post-blur, nearby-puddle
  gen / drain / water / cover, relief blur, wet copy / dim, car wetness, mirror drops (+ snow), screen combine / fade,
  racing line copy, windscreen (move, narrow wipers, normal-map gen, trace blur, refraction mask, finalize).
  Textures `rain_fx/{drops 1.4 MB, hits 66 KB, puddles 699 KB}.dds`. Physics side (cross-ref `physics_*`):
  `DISABLE_RAIN_PHYSICS`, assist `_EXT_RAIN_PHYSICS`, `Rain Grip Lat / Long` (4 values each), "AI rain grip mult.",
  `Rain Extension: Assuming slick tire / Found rain tire` (log), `AUTOSELECT_RAIN_TYRES`. Occlusion:
  `RAIN_OCCLUSION` per car (baked at load: "Calculating rain occlusion for formula_hybrid_2020.kn5: 570,890 us",
  cache `cars_data/<car>/rain_occlusion`), `RAIN_FX_OCCLUSION_HEATING(_EXHAUST)`, `_ADJUSTMENT`,
  `_CASTING_SPHERE`, `RAIN_FX_WINDSCREEN_OCCLUSION` (`POSITION`, `RADIUS`, `GRADIENT_OFFSET`, `AMOUNT`).
  **Per-track config in the DLL** (see below). `VERIFY_RAIN_FX`: **no such string in any of the three packages**
  (`dll_strings.tsv`, `extension/` of 0.2.11, preview212, preview649 searched): it is not a key of this build.
- **Embedded RainFX per-track configs.** Overlay archive 3: **100 `tracks/<id>.ini` files, 97,896 bytes**, each
  with one `[RAIN_FX]` section (the same set sits in the mirror as `internal/configs/rain.zip`). Key families (count
  of keys / files): `PUDDLES_MATERIALS` 99, `SMOOTH_MATERIALS` 99, `ROUGH_MATERIALS` 98, `SOAKING_MATERIALS` 97,
  `LINES_MATERIALS` 82, `STREAM_EDGE_n` 654 / 35 files, `STREAM_POINT_n` 239 / 17, `PUDDLES_MESHES` 17,
  `LINES_FILTER_MATERIALS` 13, `SOAKING_MESHES` 12, `ROUGH_MESHES` 9, `RELIEF_MATERIALS` 7, `SMOOTH_MESHES` 3,
  `LINES_MESHES` 2, `STREAM_WALL_EDGE_n` 4. So a track lists which materials hold puddles / soak water / are rough
  or smooth, and polylines for water streams. 19 of the 26 installed tracks are in the set (not
  `bahrain_international_circuit`, `ct_backwoods`, `ks_black_cat_county`, `ks_drag`, `monaco_2017`, `montreal`,
  `vhe_interlagos`); the loaded configs of 9 tracks carry their own `[RAIN_FX]` (keys `PUDDLES_MATERIALS`,
  `SOAKING_MATERIALS`, `SMOOTH_MATERIALS`, `ROUGH_MATERIALS`, `LINES_MATERIALS`, `*_MESHES`).
- **GrassFX.** Procedural grass generated by compute shaders (`flgGrass*` 26 names: generation, count, move,
  air, pieces, adjustments with masks, shadow pass, gbuffer, trees baking). Per track: `[GRASS_FX] GRASS_MATERIALS,
  ORIGINAL_GRASS_MATERIALS, OCCLUDING_MATERIALS(_ALPHA), SHAPE_SIZE, SHAPE_TIDY, SHAPE_CUT, MASK_*, TEXTURE`,
  `[GRASS_FX_AREA_n]`, `[GRASS_FX_ADJUSTMENT_n]`, `[GRASS_FX_TEXTURE_GROUP_n]` (`PIECE_n`, `*_CHANCE`),
  `[GRASS_FX_CONFIGURATION_A..D]`; `tracks/common/grass_fx.ini` library; textures `grass_fx/{generic 1.4 MB,
  highlands 2.1 MB}.dds`. 10 installed track configs use it. `ac.configureGrassShading` from the weather style.
- **Particles FX.** Compute particles: smoke (`pfxSmoke*`, `accBlurSmokeTiles`), marbles (`pfxMarbles*`), screen smoke,
  skid wet blur, oil spill, car flag cloth (`pfxCarFlagCloth`), a sort (`pfxCommon_sort_cs_*`); `ac.Particles.Smoke /
  Flame / Sparks / Detractor`, `ac.ParticlesMaterial`, `ac.setParticles{Smoke,Grass,GrassPieces}Material`;
  exhaust flames per car: `PARTICLES_FX_EXHAUST`, `_EXHAUST_FLAMES`, sparks `SPARKS_AS` in colliders; per track
  `[PARTICLES_FX] FIREWORKS_POS_n, SMOKE_BLOCK_START/END`, `particles_track.ini` templates (stove, bonfire, flare).
- **Tyres FX.** Flex (`FLEX_MULT`, `FLEX_PROFILE_MULT`, `FLEX_SKEW_*`), damage (`DAMAGE_FLAT_SPOT_GAIN`, `DAMAGE_GRAIN_*`,
  `DAMAGE_SPEC_MULT`, `DAMAGE_NORMALS_MULT`, `BROKEN_TYRES_BASE_NUDGE`), dirt and grass colours
  (`DIRT_COLOR_A/B`, `GRASS_*`, `SOIL_*`, `*_ALT_COLOR`, `DIRT_ALT_CONDITION=SEASON_WINTER`), tread patterns
  (`TYRES_FX_PATTERN?`, `TYRES_FX_CUSTOMTEXTURE_S/M/H`), spikes (`TYRES_FX_SPIKES_n`, shader `insTyreSpikes`),
  textures `tyres_fx/{damage_flatspot, damage_marbles, ontop_dirt, ontop_grass}.dds`. Brake discs: shader
  `accTrueEmissive_vs_brakediscfx`, `brakedisc_fx/rim_%d.dds`, `BRAKEDISC_FX_FRONT/REAR`. Cross-ref the physics
  brief (`physics_tyre_susp.md`) for the non-visual tyre model.
- **Skid marks.** `dclSkidmarks*` 16 names (compute age / fade / spawn / update, heightmap, parallax, mirror);
  the log line "Creating skidmarks renderer: 20000 particles"; saved across sessions under `cache`.

### 3.8 Post-processing, anti-aliasing, upscalers

- **YEBIS tweaks.** `yebisest.ini` is empty ("no longer doing anything"). YEBIS parameters are reachable from Lua
  through `ac.onPostProcessing(callback)` with an `ac.PostProcessingParameters` of **122 fields** (hue,
  saturation, brightness, contrast, sepia, colour temperature, white balance, 15 tonemap fields, 10 auto-exposure
  fields, vignette, chromatic aberration, diaphragm, feedback, 40 glare / bloom / star / ghost fields, 14 DOF fields,
  17 god-ray fields, lens distortion, `filmicContrast`, `customTonemappi...`), with 45 `ac.getPp*` / `ac.setPp*`
  accessors. `[sdk]` Cross-ref `docs/map/yebis.md` (rustyAC's own YEBIS map).
- **Tone mapping.** `ac.TonemapFunction` 0..16 (Linear, LinearClamped, Sensitometric, Reinhard, ReinhardLum, Log,
  LogLum, ACES, Uchimura, RomBinDaHouse, Lottes, Uncharted, Unreal, Filmic, ReinhardWp, Juicy, AgX); 10 alternative
  shaders `accAltTonemapping_ps_{aces,agx,filmic,gt,juicy,lottes,rbdh,unc2,unreal,wplb}`; custom code via
  `ac.setPpTonemapFunction({shader=...})`. HDR output in `dxgi_tweaks [HDR]`.
- **YEBIS replacement.** `[POSTPROCESSING] LIGHTWEIGHT_REPLACEMENT` of the base style (Lua + 10 `pp_*.fx` shaders:
  tonemap, bloom 4 stages, DOF, final, gamma, virtual mirror, auto-exposure 1) and `accYebisAlt*` (C++ side), "a single
  full-res pass" with colour grading, vignette, simple auto-exposure, glare, DOF, sun rays; 34 colour-grading LUT
  PNGs in `extension/textures/color_grading`.
- **Anti-aliasing.** `graphics_adjustments [ANTIALIASING] MODE`: `CMAA` (default; CMAA 2.0, compute, 5 `accCmaa_*`
  buffers), `FXAA` (original), `FXAA3` (3.11, 5 tunables), `SMAA` (3 passes, `areatexdx10.dds`, `searchtex.dds`),
  `YEBIS`, `DEFAULT`, `DISABLED`; log on this PC: "PPAA: using CMAA". MSAA custom resolve with a tonemapping fix.
  TAA only in Extra FX.
- **Upscalers** (`[FSR] OLD_IMPLEMENTATION`): 0 FSR 1.0.2, 1 an old FSR port, 2 NVIDIA Image Scaling 1.0.3, 3 DLSS
  (download, v310.9.1, NGX; presets E, F, J, K, L, M; string count about 152), 4 "Basic" (point / linear /
  bicubic / Lanczos / Catmull-Rom for comparison), 5 FSR2 2.2.1, 6 OptiScaler 0.9.2 (a 117 MB download that
  injects DX12 upscalers), 7 FSR3 3.1.2, 8 Snapdragon GSR 1, 9 Snapdragon GSR 2.0.0, 10 XeSS 2.0.2 (Intel Arc),
  11 XeSS 3.0.2, 12 FSR3/4 3.1.5 (FSR4 only on RDNA3 / RDNA4). Built into the DLL: FSR 2/3 (28 exports
  `ffxFsr2* / ffxFsr3Upscaler*`), SGSR 1/2 shaders, NIS, CAS; delay-loaded: `libxess_dx11.dll`,
  `ffx_backend_dx11_x64.dll`, `nvngx_dlss.dll`. Needs Extra FX (except the spatial ones), not VR, "single or
  triple screen only". `[NEURAL_RENDERING] ACTIVE=0` is NVIDIA's "DLSS Neural Rendering" (`DLSSNR.*` strings;
  custom DLL not shipped). Quality scale factors: 0.883 / 0.77 / 0.667 / 0.58 / 0.5 / 0.333. `[ini][dll]`
- **Motion blur.** AC's own with a gain (`[MOTION_BLUR] INTERIOR_MULT=1.4 EXTERIOR_MULT=1.2`), CSP's (Extra FX:
  `accMotionBlur*` 11 programs: tile max H/V, fill background, low / lowest / triple). **Lens flare / glare**:
  YEBIS glare plus `ADJUST_GLARE=1` ("increase glare resolution to prevent flicker").
- **Scripted filters.** `extension/lua/pp-filters` has 7 (blur, cartoon, curved monitor, planetarium, tunnel, vhs,
  vintage; each `manifest.ini` + `filter.lua` [+ `filter.fx`]); SDK `ac_pp_filters`; options
  `[SCRIPTABLE_FILTER] ENABLED_MAIN / _TRACK / _REPLAYS / _PAUSE / _PITS`; LDR image only.

### 3.9 Lua API groups relevant to rendering (counts from `lua_sdk_functions_union.txt`, 2,729 names)

| Group | Count | Notes |
|---|---|---|
| `ac.set* / get* / calculate* / fix* / adjust*` for sky, light, fog, cloud, PP, time (regex match) | ~256 | only in `ac_wfx_impl` (WeatherFX style), a subset in `ac_apps` / `ac_tools` (time offset, VAO mode) |
| `ac.SceneReference` methods | 110 | `loadKN5(Async)`, `findMeshes/Nodes`, `createMesh`, `setMaterialProperty`, `setMaterialTexture`, `applyShaderReplacements`, `createFakeShadow`, `setShadows`, `raycast`, `excludeFromCubemap`, `projectTexture` |
| `render.*` | 39 (+ 6 PositioningHelper) | `render.on('main.track.transparent', fn)`, `shaderedQuad`, `fullscreenPass`, `mesh`, `glBegin/Vertex`, `setBlendMode/Cull/Depth`, `tornado`, `debug*` |
| `ac.GeometryShot` | 35 methods | offscreen scene render with options (`setShadersType`, `setSky`, `setParticles`, `setOriginalLighting`, `setAmbientColor`, `setExposure`) |
| `ac.TrackPaint` | 26 | paint onto the track surface |
| `ac.ColorCorrection*` | 14 methods, 18 constructors | per-pixel colour operations used by weather and filters |
| `ac.LightSource` | 3 methods, 32 fields | dynamic light from Lua |
| `ac.SkyCloudV2 / SkyCloud / SkyCloudMaterial / SkyCloudsCover / SkyCloudMapParams / SkyExtraGradient` | 6 classes | clouds and sky gradients |
| `ac.Particles.*`, `ac.ParticlesMaterial` | 4 + 1 | emitters |
| `ui.ExtraCanvas`, `ui.renderShader` | 21 + | shaders from Lua (HLSL snippets with `PS_IN`); used by pp-filters and scripted displays |

## 4. Files it reads

- **Defaults:** `<AC>/extension/config/*.ini` (57 top-level ini + 9 `data_*.txt`; 25 are the modules of 3.1), `config/cars/{generic 3,
  common 29, loaded 9 (8 car ini + storage.data)}`, `config/tracks/{generic 1, common 16, loaded 35 (11 ini + 23 helper .kn5 + storage.data)}`,
  `extension/weather/base` (57 files), `extension/weather-controllers` (13 files), `extension/lua/pp-filters`
  (7 filters), `extension/textures/color_grading` (34 PNG), `extension/vao-patches` (10) and `vao-patches-cars` (7),
  `extension/textures/cars/lotus_exos_125_s1.zip` (5.2 MB, CM download), `extension/internal/shader-libs/sky.hlsl`
  (11 KB: the public sky model include, cbuffer `cbSkyData` at register b6, 16 extra gradients, star map at t12),
  `extension/internal/lua-sdk/*`.
- **From the game:** `content/weather/<name>/{weather.ini, colorCurves.ini}`, `system/cfg/{lighting, colorCurves, graphics,
  hdr, ppfilters/*}.ini`, `cfg/video.ini` (`[POST_PROCESS]`, `[VIDEO] SHADOW_MAP_SIZE, AASAMPLES`, `[CUBEMAP]`,
  `[MIRROR]`, `WORLD_DETAIL`), `cfg/race.ini` (3.3), `content/tracks/<t>/{ui/ui_track.json (geotags), data/lighting.ini,
  extension/ext_config.ini}`, `content/cars/<c>/extension/ext_config.ini`.
- **Documents:** `cfg/extension/<module>.ini` (overrides), `cfg/extension/state/lua/wfx_impl/<style>.ini`
  (style settings and cached values), `logs/custom_shaders_patch.log`.
- **Cache** (`<AC>/cache`): `clouds.bin` 44.6 MB ("Clouds: loading cached"), `csp_configs` (config cache), `cars_data`
  (rain occlusion, fake shadows, per-car data), `lua_shaders`, `meshes_metadata`, `trees_alignment`, `track_logos`.
- **DLL overlay (read through the zip central directory, plain deflate, none encrypted):** archive 1 shaders, 3 RainFX
  configs, 4 Lua libraries (LuaJIT bytecode, 135), 7 the 166-entry mirror including `internal/lua-module/*.luac`
  (WeatherFX style previews, plugins), `internal/shader-tpl/*.fx` (13 shader templates for Lua shaders),
  `internal/tzdata`, `textures/*` (PBR, smaa, weather_fx, rain_fx, tyres_fx, grass_fx, particles_fx).

## 5. How it is switched on

- **Master:** `general.ini [BASIC] ENABLED=1` (everything). Each module: `ENABLED` where documented as active
  (`weather_fx`, `rain_fx`, `extra_fx`, `particles_fx`, `grass_fx`, `smart_shadows`, `skidmarks_fx`, `tyres_fx`,
  `windscreen_fx`, `nice_screenshots`, `surfaces_fx`); no-op `ENABLED` in `lighting_fx`, `reflections_fx`,
  `fake_shadows_fx`, `colorful_shadowing`, `brakedisc_fx`, `shadowed_wheels`, `graphics_adjustments`,
  `track_adjustments`, `smart_mirror` (an `ENABLED2=0` line "not supported and will break things" is the only way).
- **Per car / per track:** `generic.ini` for all; `loaded/<id>.ini` and `ext_config.ini` per item; `[BASIC]
  NO_NEED_FOR_FIXES=1` stops the generic fixes; cars: 8 CM configs here, 20 with `ext_config.ini`; tracks 11 CM
  configs, 3 with `ext_config.ini`.
- **Gates:** Extra FX and linear colour space need `[POST_PROCESS] ENABLED=1`; Extra FX not with VR, not with AC's own
  motion blur; custom modes need FXAA on; DXGI tweaks off with "MSAA without post-processing".
- **On this PC** (`video.ini` post-processing 0, FXAA 1 in the section, AASAMPLES 4, MSAA, `WORLD_DETAIL=0`,
  cubemap size 0, shadow size -1; graphics card AMD Radeon Pro 5500M in a MacBookPro16,1 under Windows): disabled
  by the log: `DXGI Tweaks`, `New Modes`, `Walking Out`, `Custom Rendering Modes`, `Mumble Plugin`. Running:
  WeatherFX (style `base`, controller `base`, legacy no-LCS path), CMAA, skid marks, rain occlusion baking,
  VAO patches, smart shadows ("Changing shadow resolution: 0, splits=4"), Lighting FX, RainFX. Not running (off or
  no post-processing): Extra FX, TAA, SSLR, SS lights, volumetric lights, upscalers, linear colour space.

## 6. Size

| Item | Number |
|---|---|
| Module files | 25 files, 977 keys, 103 sections, 103,808 bytes (3.1); all of `extension/config/`: 57 ini + 9 txt top level |
| Per-car / per-track libraries | cars `common` 29 files (269 KB), `generic` 3, `loaded` 9; tracks `common` 16 (185 KB), `generic` 1, `loaded` 35 (65.7 MB, 23 helper kn5) |
| Section vocabulary in the installed configs | 418 distinct normalised section names; `MATERIAL_ADJUSTMENT` 331, `LIGHT_SERIES` 248, `SHADER_REPLACEMENT` 158, `CONDITION` 139, `LIGHT` 57, `FAKE_SHADOW` 38, `MODEL_REPLACEMENT` 26 |
| WeatherFX | 57 style files (25 Lua, 27 shader, 3 data, 2 meta), 13 controller files; 36 weather definitions; 33 types + 1; 34 LUT PNG |
| Lua SDK for rendering | `ac_wfx_impl` 2,082 functions (1,287 own, 115 classes), `ac_wfx_controller` 919 (124 own), `ac_pp_filters` 1,699 (904 own), `ac_scripted_texture` 1,558 (763 own); union 2,729 |
| Shaders in the DLL | 6,683 DXBC blobs, 123.2 MB uncompressed (30.9 MB compressed), hashed names; 694 `.fxo` names in the image strings (620 programs) |
| Embedded configs | RainFX 100 files, 97,896 B; `data_track_params.ini` 435 tracks, 46,905 B; `data_gbuffer.ini` 23 rules |
| Textures in the mirror | `weather_fx` 4 (starmap 22.4 MB, earth 2.8, moon 1.4, iss), `rain_fx` 3, `tyres_fx` 4, `grass_fx` 2, `particles_fx` 2, `common` 15 (PBR 8 x 1.4 MB, water normal 5.6 MB), `smaa` 2 |
| VAO | 10 track patches (29.8 MB) + 7 car patches (1.4 MB), all plain zip |
| Hooks (renderer group) | 368 rows: 301 hooks, 67 lookups |
| Modules (C++) | 40 `ac_ext_*` module classes, 47 settings structs, 36 module ids |
| Owner | 14 files in `cfg/extension` (3 empty: `general`, `gui`, `weather_fx`; 11 with content, incl. `new_behaviour` and `window_position`), `state/` 10 entries |

## 7. Port difficulty for rustyAC

What rustyAC has (`docs/port`, `crates/rustyac-render` = D3D11 through the `windows` crate, 1:1 port of kgl,
`GraphicsManager`, materials, scene graph, three shadow cascades, `colorCurves` lighting, sky dome and v1 clouds,
`SunAnimator` with the +/-80 clamp, static and dynamic cube maps, mirrors (HQ too), car lights as emissives,
flames v1, tyre smoke, skid marks, fake shadows, MSAA, FXAA, motion blur, the HUD). It loads the game's own `.fxo`
shaders. It has **no** YEBIS (Task 23b is a plan: `docs/port/postfx.md` section 8), no compute / UAV use (grep of
`kgl.rs`), no dynamic lights, no G-buffer, no Lua host, no INIpp, no per-material replacement. D3DCompile is
reachable (the `Win32_Graphics_Direct3D_Fxc` feature is on). The browser build has its own wgpu copy: any CSP
feature would be a second implementation there.

| Module / part | Size | What exists to reuse | What is entirely new | How to prove it |
|---|---|---|---|---|
| Config machinery (INIpp, `ext_config.ini`, includes, conditions, generic / loaded) | **M** | `INIReader` port, content loader | the language (mixins, templates, generators, Lua-expression evaluation: a small interpreter or LuaJIT), `CONDITION` inputs and LUTs | expand every shipped config and compare section dumps with CSP's own `ConfigsCache` output; golden files |
| Graphics Adjustments (LOD multipliers, visible-car limits, adaptive clip planes, origin shift, render scale) | **S-M** | `CarLodManager`, projection code | clip-plane rules, origin shift, resolution scale; upscalers see below | scene hash / command-log diff against CSP is impossible; compare the matrices and plane values with CSP's `ac.getCameraParameters` logged by a Lua app |
| True night (3.4 night-lite) | **M** | `SunAnimator`, `cbLighting`, `colorCurves` path | unclamped angle, astronomy (own implementation of standard formulas), night colour table, moon + stars sky layer, brightness multiplier | image oracle (below) at fixed `ac.setWeatherTimeOffset`; style's published numbers as spec |
| Faithful WeatherFX style | **XL** | sky and cloud ports as the base | sky V2 shader, clouds V2 + noise generation, cloud shadows, fog model, light pollution, aurora, lightning, 24 h time + seasons, controllers (3 small scripts), Lua host or native port of ~250 setters | call-trace oracle: log every `ac.set*` of the real style (a wrapper style) and compare with the port; images second |
| Lighting FX (dynamic lights, series, car lights, bounce, shadows) | **XL** | emissive lights of the car, fake shadows | light buffer and clustering, replaced `ne*` / `sm*` shaders that read it, exponential shadow maps, light map for bounce, conditions | per-light parameter dump (CSP Debug "Log lights") vs port; images in a dark scene |
| Materials / shader replacement / PBR / car paint | **L-XL** | `Material`, `KN5IO`, shader loading | filter language, variable overrides, replacement shaders (CSP's own DXBC from the player's `dwrite.dll` or recompiled from the public HLSL), texture slots | render a car with the replacement and compare to a CSP screenshot at the same pose (a CSP image oracle); property tables from the log |
| Model replacement / mesh adjustment / smooth normals / track occluders | **M** | `KN5IO` | insert and hide nodes, `INSERT_AFTER` ordering | node-tree dump after load vs CSP's `ac.SceneReference` dump from a Lua tool |
| Smart Shadows | **M** | 3-cascade port | automatic split rules, 4th cascade, stabilisation, smooth overlap, lazier update | shadow-map matrices and splits dumped from CSP (`CameraShadowMapped` values) vs port |
| Fake shadows / colorful shadowing / shadowed wheels | **S-M** each | `CarFakeShadow` | projection onto the ground (Extra FX), baking with many lights, coloured shadow path | compare the baked texture files in `cache/cars_data` |
| Reflections FX (partial reprojection, IBL prefilter, local cubemaps, refracting headlights) | **L** | cube map, mirror ports | compute prefilter, reprojection, local probes | cube-map readback image vs CSP |
| Extra FX (G-buffer, TAA, SSLR, SSAO, SS lights, volumetric, fog blur) | **XL** each | none | a deferred G-buffer pass for the whole scene, compute | none good: per-effect screenshots; needs post-processing on in the reference |
| RainFX (+ wipers, windscreen, wet roads, per-track configs) | **XL** | smoke / particles, mirrors | ~60 compute and pixel programs, per-track tables, wet-surface shading, physics link | per-track config parse tests; image compare in a fixed rain state (`ac.setRainAmount` etc.) |
| GrassFX | **L** | none | compute generation, track masks, deformation | images at a fixed place |
| Particles FX (GPU smoke, sparks, flames v2, pieces, marbles, fireworks) | **L** | vanilla `ParticleSystem` | compute simulation + sort | statistical / image compare (random) |
| Skidmarks FX / Tyres FX / brake discs / windscreen FX | **M** each | `SkidMarkBuffer`, `BrakeDiscGraphics`, `TyreBlur` | new buffers, damage textures, flex | per-item images at a fixed pose |
| Post-processing (YEBIS 23b first), alt tonemappers, `accYebisAlt`, scripted filters | **XL** (YEBIS), **M** (tonemappers) | `CameraForwardYebis` frame | the 17 tone maps (public maths), the pp-filter Lua host | images vs CSP with the same filter |
| AA modes (CMAA2 compute, SMAA, FXAA 3.11) | **M** each | FXAA port | compute CMAA, SMAA tables | image diff on a fixed frame (deterministic) |
| Upscalers (FSR 1, NIS, SGSR 1 spatial; FSR2/3, SGSR2 temporal; DLSS, XeSS, OptiScaler) | **M** (spatial), **XL** (temporal, vendor DLLs; not available in the browser) | none | motion vectors, jitter, history | image diff for spatial ones only |
| Nice screenshots (accumulation) | **S-M** | screenshot code | sub-pixel jitter loop and accumulation target | CSP screenshots of the same pose |
| Custom rendering modes (360, fisheye) | **M** | six-face render exists for cube maps | the projection passes | image diff |
| Smart mirror (real mirrors, virtual mirror PP) | **M-L** | mirror ports | per-mirror cameras, projection modes | mirror texture compare |
| VAO patches | **M** | KN5 loader | zip + `Patch_v4/v5` layout (to be decoded), apply by mesh order | compare per-vertex AO against the data the log reports (mesh count per node: "1125 meshes") |
| Wipers / reverse lights (config side) | **S-M** | emissive channels, `ksanim` | `ANIMATED_WIPER` maths, conditions | images at fixed `wiperProgress`; gear R |
| DXGI tweaks, HDR output, window options | **S** | window code | flip model, VRR, HDR10 | not image-testable |

**A CSP image oracle (none exists; proposal).**
1. *Reference runs of the real AC + CSP* on the same PC with post-processing both off (the owner's mode, which
   keeps WeatherFX in its legacy path and Extra FX off) and on (with `[POST_PROCESS] ENABLED=1`, Extra FX, LCS):
   a **fixed replay** (or a stopped session), a fixed free camera, `ac.setWeatherTimeOffset(x, true)` /
   `ac.setWeatherExactUTC0Timestamp` for the time, `ac.overrideReplayConditions` for the conditions,
   `nice_screenshots` accumulation off, F8 screenshots into `Documents/.../screens`. CSP Debug already offers: time
   offsets, VAO modes, light outlines and "debug lights", "debug rain occlusion", rain racing line, step back,
   reset, LOD switching.
2. *Deterministic captures inside CSP:* a small Lua tool (CSP loads tools from `extension/lua/tools` and apps) that
   uses `ac.GeometryShot` (35 methods: set camera, sky, particles, shader type, ambient, save) so the picture is
   made without UI and at a chosen resolution. Cloud placement uses `math.random()` with LuaJIT's default seed,
   so cloud layouts may differ between runs: record them or fix the seed in a wrapper style.
3. *Call-trace oracle (cheapest and most precise for WeatherFX and lights):* install a copy of the style that
   wraps the global `ac` table, logs each `ac.set*` call with arguments and the frame's `ac.getSim()` time through
   `ac.log`, then loads the original script; the log is in `custom_shaders_patch.log`. Equivalent for lights: a
   tool that dumps each `ac.LightSource` field set. The rustyAC port then must produce the same stream for the same
   conditions, exactly like `gpulog` for Direct3D. This needs no pixel agreement and works with hardware GPUs.
4. *Direct3D command log of CSP itself:* possible in principle (CSP drives the game's own device; `gpulog.rs`'s
   stand-in context could sit between), but `dwrite.dll` needs a real `acs.exe` process, `acs.pdb`, dbghelp and
   its own threads; `attach.md` 1.1 / 7. Treat as a research task, **high risk**.
5. Pixel comparison must respect: GPU (hardware, not WARP: CSP's compute shaders), TAA jitter (off), cloud noise,
   time-dependent random seeds, "never a hardware compare beside a WARP batch" (project note).

**Order hint for a port plan** `[interp]` (ranking is done elsewhere): config machinery -> materials / adjustments
(emissive switching with conditions) -> night-lite -> Smart Shadows -> reverse lights and wipers (config side) ->
Lighting FX -> RainFX -> the rest.

## 8. Open questions and not mapped

- **The astronomy.** The code that turns date, time, latitude, longitude into sun and moon directions, phase and
  eclipse angle sits in the dll; no plain Lua names for the `ac.*` bindings appear in `.rdata` (the Lua names are
  not stored as strings; only `lj_*__gp` style exports for cameras exist), so I could not find the C++ behind
  `ac.getSunDirectionTo`. `Weather FX: start time` is in function 0x13310e0 (not decompiled).
- **Where `__CM_UNCLAMPED_SUN_ANGLE` is read.** The string (0x2297878) has no code reference by `lea` in
  `dll_func_strings.tsv`; its consumer may be in 0x11e82b0 (reads `SUN_ANGLE`, `TIME_MULT`,
  `__CM_WEATHER_CONTROLLER`, `__CM_DATE`) which was not decompiled.
- **What the handler chains hold.** The vectors at 0x1826974c8 (`SunAnimator::update`), 0x182697570 (lighting
  post-update) and 0x182697a30 (`SkyBox::render`) are filled by registrations I did not find; their entries are the
  module hooks.
- **Binary formats not decoded:** `Patch_v4/v5.data`, `ExtraSamples.data` of the VAO patches; the cloud cache
  `clouds.bin`; the symbol / config caches.
- **Meaning of the shader prefixes** `ne*` and `sm*` in replacement shader names; the HLSL sources are public
  (the archive comment says so) but were not looked at.
- **`VERIFY_RAIN_FX`** (named in the task): not in this build (3 packages and the dll string table searched).
- **`CONTROLLER=sol`** is the shipped default but no `sol` controller exists in the package; the fallback chain
  (launcher key, then `base`?) is inferred from the log, not read from code.
- **`WEATHERFX_V1/V2/V3`** meaning is a guess (online protocol versions).
- Numbers of the vanilla colour-curve "low" values at sun angles 80..180 (needed for a night colour table) were not
  extracted; they are in `content/weather/*/colorCurves.ini` and `system/cfg/colorCurves.ini`.
- The `[ini]` counts "hidden" are regex-based and approximate; "keys" are exact line counts of uncommented keys.
- The owner's `race.ini` start time (SUN_ANGLE -16.00 now vs "23:20:36" in the last log) shows the file changed
  between the last logged session and today; I did not reconcile which angle -> time formula CSP uses
  (vanilla: 13:00 +/- ... not measured).

## Evidence

- `<AC>/extension/config/{lighting_fx, weather_fx, rain_fx, reflections_fx, extra_fx, particles_fx, grass_fx,
  smart_shadows, fake_shadows_fx, colorful_shadowing, skidmarks_fx, tyres_fx, windscreen_fx, brakedisc_fx,
  shadowed_wheels, graphics_adjustments, custom_rendering_modes, yebisest, dxgi_tweaks, nice_screenshots,
  track_adjustments, smart_mirror, car_instruments, small_tweaks, surfaces_fx, data_gbuffer, data_manifest}.ini` `[ini]`
- `<AC>/extension/config/{cars,tracks}/{generic,common,loaded}` `[ini]`; `extension/weather/base/**`,
  `extension/weather-controllers/**` `[lua]`; `extension/internal/lua-sdk/{ac_wfx_impl, ac_wfx_controller,
  ac_pp_filters}/lib.lua` and `ac_apps/lib.lua` `[sdk]`; `extension/internal/shader-libs/sky.hlsl`;
  `extension/vao-patches*/` (zip listing, `Config.ini` read) `[dll]`
- `Documents/Assetto Corsa/cfg/extension/*.ini`, `cfg/race.ini`, `cfg/video.ini`, `logs/custom_shaders_patch.log`
  `[log-csp]`
- `re/scratch/csp1/`: `dll_strings.tsv` (RTTI class names, `.fxo` names, `__CM_*` keys, strings at the RVAs above),
  `dll_func_strings.tsv`, `dll_zips_all.json` + a read of `dwrite_copy.dll` (archive 3 RainFX configs; archive 7
  `config/data_track_params.ini`, `internal/configs/rain.zip`, textures listing), `hook_groups.tsv`,
  `csp_hook_table.tsv`, `decomp_hooks/{1121f60, 1121ec0, 1121dc0, 112f9a0, 11c5670, 117f630, 117f2d0, 414fc0,
  11bd8b0, 6915c0}.c`, `decomp_req1/1387350.c` (strings only), `attach.md` 1.7 and 2, `lua_sdk_counts.tsv`,
  `lua_sdk_functions_union.txt`, `content_features.json`
- rustyAC: `docs/atlas/renderer.md`, `docs/port/{renderer_core, renderer_scene, renderer_car, renderer_leftovers,
  postfx, damage_sparkles}.md`, `crates/rustyac-render/src/{sky.rs, lighting.rs}` (the +/-80 clamp, the
  low / high colour blend), `docs/csp/content_needs.md`, `brainstorm.md` item 2.17
- Other packages: `rustyAC/ignored/CSP/lights-patch-v0.2.11`, `...preview212`, `...preview649` (module list diff:
  `rain_fx.ini` and `data_paintable.ini` appear in preview212, `base-schedule` in preview649)

## Decompile requests

1. `0x11e82b0` - race.ini `[LIGHTING]` / `[WEATHER]` reader (strings `SUN_ANGLE`, `TIME_MULT`,
   `__CM_WEATHER_CONTROLLER`, `__CM_DATE`): how angle, date and controller turn into the WeatherFX timestamp; is
   `__CM_UNCLAMPED_SUN_ANGLE` used here?
2. `0x13310e0` - "Weather FX: start time=": start time, date, `__TRACK_TIMEZONE_*` handling and the
   `data_track_params.ini` fallback.
3. `0x132f3b0` - `__CM_WEATHER_TYPE` / `__CM_WEATHER_CONTROLLER` consumer: type to conditions, controller choice and
   the `sol` fallback.
4. `0x1333070` - "Weather FX: reload settings": the style / controller load order and live reload.
5. xrefs to `0x1826974c8`, `0x182697570`, `0x182697a30`: the handler registrations (who is in each chain; which
   one implements the sun and which applies `ac.setLightColor`).
6. `0x1509f10` - the function that names `lighting_fx.ini`, `reflections_fx.ini`, `fake_shadows_fx.ini`,
   `colorful_shadowing.ini`, `brakedisc_fx.ini`, `shadowed_wheels.ini`, `track_adjustments.ini`, `smart_mirror.ini`:
   the module list and where `ENABLED` is read.
7. `0xf95550` - references `graphics_adjustments.ini` and `custom_rendering_modes.ini` (module settings loader).
8. `0x1326f10`, `0x139e4e0`, `0x14818b0` - `weather_fx`, `WEATHER_FX`, `PARTICLES_FX`, `SMART_MIRROR` string users.
9. `0x11e4270` - "Applying VAO patch" (and `0x1014f40`, "VAO patch file is damaged"): the layout of
   `Patch_v4/v5.data` and how mesh order is matched.
10. `0xb8cd80` - "Extra VAO samples are set, second version": the `ExtraSamples.data` layout.
11. `0xda6c40`, `0xdaa260`, `0xdaedf0` - RainFX baking and `rain_occlusion/%s` cache: occlusion data format.
12. `0xcd1f70`, `0xce1d10`, `0x12c1bd0` - `GRASS_FX` section readers: grass key semantics.
13. `0x362bd0`, `0x6fa2e0`, `0xc8d370`, `0xdabbd0` - `RAIN_FX` section readers (track and car).
14. `0x1220210` - "Fake shadows for": the fake-shadow bake (`LIGHTS_COUNT`, projection).
15. `0x91e2d0` - "Rain Extension": rain-tyre detection and the rain grip law (physics cross-reference).
16. `0x1228e20` - "PPAA: using ...": the AA mode selection (CMAA / FXAA3 / SMAA / YEBIS).
17. `0xdebcf0` - reflections `cbuffer_ibl`; `0x1254b10`, `0x12e8c40`, `0x12eded0` other `REFLECTIONS_FX` users.
18. `0x73a890`, `0x71a520` - "Lights map params", "Light-dark exposure border": light map and auto-exposure of
    Lighting FX / Extra FX.
19. `0x14ed8f0` - "Horizon tube candidate": detection of water / horizon meshes (what is replaced).
20. `0x313e70` - "Gaussian splats are configured": the config key and format for splats.
