# Rendering (overview)

CSP's biggest area by volume: a second render pipeline wrapped around `acs.exe`'s Direct3D 11
renderer, a material and shader replacement system, a lights system, WeatherFX (sky, sun, moon,
true night, clouds), RainFX and wet roads, post-processing and upscalers, and a configuration
language (INIpp) that drives all of it from `ext_config.ini` files. Three detail files carry the
evidence; this file is the index and the summary.

| File | What it holds |
|---|---|
| [`rendering_config.md`](rendering_config.md) | the ~25 graphics modules and their keys; WeatherFX architecture and **true night** (how CSP goes beyond `acs.exe`'s clamp of the sun angle at 80 degrees: astronomy, moon as scene light, star map, fake exposure); lighting and materials; shadows, reflections, RainFX, GrassFX, particles, post-processing and upscalers; INIpp; per-module port difficulty |
| [`rendering_core.md`](rendering_core.md) | the hooks in the scene graph, materials, shaders, `kgl` / D3D11, YEBIS post-processing, particles (`KunosSim.lib`, `kgl.lib`, `ksCameraYebis.lib`, `ksParticleSystem.lib`) |
| [`rendering_content.md`](rendering_content.md) | the content-side hooks: `CarAvatar`, `TrackAvatar` and ~35 small objects (flames, tyre smoke, skid marks, mirrors, sun animator, weather manager, damage, lights) |
| [`vr_screens.md`](vr_screens.md) | VR, triple screen, windowing |

## What it does, in six sentences

1. A **new material system**: every mesh / material of a car or track can be replaced or adjusted
   from `ext_config.ini` (shader, blend mode, cull mode, depth, alpha, LODs, PBR v2 carpaint, glass,
   interior, metal), applied at load time through the `CarAvatar` / `TrackAvatar` / `KN5IO` /
   `Material` hooks and rendered by CSP's own shaders (6,683 precompiled DXBC shaders in the DLL;
   the HLSL source is public).
2. A **lights system**: dynamic point / spot / line lights with shadows, light series that switch on
   the `SUN` condition, adaptive emissives, refracting headlights, bounced light.
3. **WeatherFX**: a time model with real astronomy, weather types (57 files), colour grading, clouds,
   fog, seasons and three controllers (`base`, `base-live`, `base-schedule`) written in Lua;
   **true night** is part of the default style.
4. **RainFX**: rain drops on screen and windscreen, wipers, puddles, wet-road materials, per-track
   configs (101 packed in the DLL and verifiable online), tied to the physics wet-grip law.
5. **Post-processing and output**: YEBIS tweaks, anti-aliasing modes, FSR 2 / 3, DLSS and XeSS
   integration, HDR path, custom rendering modes, DXGI tweaks, VAO patches (baked ambient occlusion).
6. A **graphics adjustments** layer (LOD multipliers, visible-car limits, adaptive clip planes,
   origin shift, render scale).

## What it replaces or extends

About 368 registration rows of the hook table are rendering classes (rows in `hook_groups.tsv`
group `rendering_hooks`): `KunosSim.lib` 156 hooks (GraphicsManager 22 + 14 lookups, Material 8,
CBuffer 7, Node, Mesh, SkinnedMesh, ResourceStore, KN5IO, CameraShadowMapped, ...), `kgl.lib` 10,
`ksCameraYebis.lib` 19 (hook counts as first extracted; 92 rows were later reclassified from lookup to hook across the whole table, see [`attach.md`](attach.md)); the content side is 118 hooks + 22 lookups in
`CarAvatar`, `TrackAvatar` and ~35 smaller car / track objects. CSP does **not** patch the D3D11 context vtable: it hooks
`D3D11CreateDeviceAndSwapChain`, reads the game's own device through data-symbol lookups and
drives it itself ([`attach.md`](attach.md) section 1.8).

## How it is switched on

`general.ini [BASIC] ENABLED=1` (everything), then one `ENABLED` key per module in its own ini, then
per car / per track `ext_config.ini` sections gated by `ACTIVE=` expressions. Post-processing is a
user setting; some modules (`custom_rendering_modes`, `dxgi_tweaks`) only run when it is on.
On this PC the owner overrides visual keys only (lighting FX, particles and brake disc FX are
switched off in his files).

## Port difficulty

**XL as a whole**; **M** for the configuration machinery and for true night; **L** for materials /
lights; **XL** for WeatherFX with the full style and for RainFX. rustyAC's renderer already
has Rust twins of every hooked class with a draw-for-draw oracle against `acs.exe`; what is new
is a G-buffer, dynamic lights, a Lua host, INIpp and per-material replacement; there is no
YEBIS port yet (`docs/port/postfx.md`). There is **no CSP image oracle**: the plan
([`plan.md`](plan.md), CSP-13 / 14) records AC + CSP screenshots at fixed weather times and compares
them with `render_oracle`. For the owner's content the visual features unlock look only
([`content_needs.md`](content_needs.md) section 4).

## True night in short (brainstorm 2.17)

Plain AC clamps the animated sun angle to +-80 degrees, so the darkest moment is dusk.
CSP's default style replaces the clamp by a real clock and date: the moon becomes the scene light
when the sun is below the horizon, the sky gets stars, planets and a moon texture that fade in,
ambient and sky colours follow night tables, and a "fake exposure" raises emissives and lights as
the scene darkens. rustyAC needs: an unclamped sun angle, a small astronomy module, a night colour
table, a moon / star sky layer, an exposure multiplier and light-series switching on the `SUN`
condition (about 88 degrees). Details and constants are in `rendering_config.md` section 3.4.
Wipers and reverse lights are content-side (`ANIMATED_WIPER`, `EMISSIVE_REVERSE` sections and the
`CarAvatar` hooks in `rendering_content.md`).
