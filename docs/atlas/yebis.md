# YEBIS 2 (post-processing): a map

> **Task 38c:** the `re/scratch/task…` paths below are history: those folders were scratch and
> most were deleted in the disk cleanups of 2026-10-15 and 2026-10-18. Every proof of this report
> reruns from `tools/proofs` (`sh tools/proofs/run_all.sh`); `docs/port/proofs.md` lists each path
> and what replaces it.

Source: `acs.exe` (the build of 2018-01-10, 22,890,776 bytes) and `acs.pdb`, read only through the local
tools (`tools/disasm.py`, `tools/re_query.py`, the PDB's public symbols and section contributions through
DIA), plus a read-only look at `system/cfg/ppfilters` and `system/shaders/win` in the game folder. Three
reader briefs with their scripts are in the git-ignored `re/scratch/task23/` (`yebis_1_compilands.md`,
`yebis_2_frame.md`, `yebis_3_params.md`); this map is their summary. **No YEBIS code is ported.** No
bytecode and no disassembly listing is in this repository.

Words: **confirmed** = read in the disassembly or measured with the render oracle. **Not verified** =
inferred from names or from the order of call sites.

## 1. What it is

YEBIS 2 is Silicon Studio's post-processing library. In `acs.exe` it is `ppfx_dx11_x64.lib`, linked
statically, namespace `PPFX`, built on 14 July 2014 (a `__DATE__` string inside it). The game calls it only
with `cfg/video.ini [POST_PROCESS] ENABLED=1`: then `Sim::createCamera` 0x1401982e0 makes a
`CameraForwardYebis` 0x140025db0 instead of the plain `CameraForward`, the scene is drawn into a 16-bit float
target instead of the screen, and YEBIS turns that picture into the final one: tone mapping with automatic
exposure, glare (bloom, star streaks, lens ghosts, anamorphic flare, afterimage), light shafts ("rays of
god"), depth of field, heat shimmer, chromatic aberration, lens distortion, vignette, feedback (a trail of
earlier frames) and a colour matrix (hue, saturation, brightness, contrast, sepia, colour temperature, white
balance).

What is **not** YEBIS although it lives in the same frame: the game's own motion blur
(`CameraForward::renderBlurred` / `solveBlur`), its own FXAA pass (`ksFXAA`), the multisample resolve and the
depth resolve (`ksMSDepthResolve`). Those are Kunos code, ported in Task 23 (`docs/port/postfx.md`). YEBIS
has motion blur, FXAA, ambient occlusion, a colour-grading table, sensor noise and auto focus too; the game
never switches them on.

The PDB has the library's **public symbol names** (3,186 of them: `PPFX::CPostEffect::ApplyEffects`,
`PPFX::CRenderGlare::GenerateGlare_Ghost` ...) and no function or type records: no argument types beyond the
decorated names, no struct layouts, no local names.

## 2. Size: every compiland

Sizes are the PDB's section contributions of each object file; functions are distinct code addresses
(identical functions folded by the linker count once). 43 objects: 28 with code, 15 with data only.

| compiland | functions | code bytes | read-only data | what it is (from its function names) |
|---|---:|---:|---:|---|
| GPUTextureUtil.obj | 236 | 201,246 | 43,220 | `CTextureUtil` + `TEXSHADERSET`: the full-screen-pass toolbox (box, gaussian, resample and bokeh blurs, tone-map passes, lens distortion, heat-shimmer particles, kernel weights, procedural textures); owns the effect that holds every shader |
| GPUPostEffect.obj | 526 | 132,077 | 42,440 | `CPostEffect`: the frame's pipeline and all parameter state (`Initialize` 0x1403bbb80, `ApplyEffects` 0x1403c5cd0, `EndPostEffectScene_InternalProcess` 0x1403b5f40, the colour-matrix maths) |
| GPUDepthOfField.obj | 107 | 81,501 | 16,668 | `CDepthOfField`: aperture filter levels, scatter bokeh, dilate, composite |
| GPUMath.obj | 109 | 61,914 | 4,575 | matrix / vector / colour / half-float maths, lens optics (a 16 kB ray tracer for lens discs, Fresnel, the photopic curve, f-number maths) |
| GPURenderGlare.obj | 93 | 46,711 | 9,064 | `CRenderGlare`: bright pass, bloom, star, ghost, anamorphic flare, afterimage, light shaft, composite, the final tone-map pass (`TonemapToSurface` 0x14041d7f0) |
| TinyEffect.obj | 110 | 27,490 | 16,528 | `CTinyEffect`: the reader and run time of the shader container (section 4) |
| TFXString.obj | 104 | 23,607 | 4,712 | the library's own string class |
| ppfx.obj | 391 | 19,724 | 5,252 | the public interface: `IPfxBaseContext` (339 names, mostly setters that hand on to `CPostEffect`), `IPfxContext_Dx11::Instantiate` 0x14039e5d0 |
| GPUInterfaceDevice.obj | 117 | 15,525 | 13,268 | `IGPUDevice`: its own wrapper of the Direct3D 11 device with a state cache |
| GPURenderTexture.obj | 51 | 15,125 | 4,824 | render-target textures with tiles and downsample filters |
| GPURenderTextureArray.obj | 28 | 11,117 | 4,544 | chains of mip / blur levels |
| GPUTextureLuminance.obj | 47 | 9,353 | 3,484 | luminance analysis and its read-back to the CPU (automatic exposure) |
| GPUUtil.obj | 30 | 6,723 | 4,969 | format and multisample helpers, error strings |
| GPUInterfaceSurface.obj | 26 | 5,902 | 3,812 | render-target and depth views |
| GPUInterfaceBuffer.obj | 23 | 4,858 | 3,980 | vertex / index buffers, input layouts |
| GPUInterfaceTexture.obj | 39 | 4,838 | 3,816 | 2D / volume / cube textures |
| GPUInterfaceResource.obj | 17 | 4,391 | 584 | resource descriptions (the library was written on a Direct3D 10 style layer) |
| GPURenderTexturePool.obj | 20 | 3,769 | 1,820 | pooled temporary targets |
| GPUGlareDef.obj | 25 | 3,698 | 1,252 | glare and star shape definitions (the presets `GLARE SHAPE` picks from) |
| GPUInterfaceEffect.obj | 50 | 3,555 | 1,916 | technique / parameter interface over `CTinyEffect` |
| ppfxRenderToTexture.obj | 40 | 3,495 | 3,144 | the render-to-texture objects of the public interface |
| GPUResourceManage.obj | 22 | 2,825 | 1,692 | device-lost registry |
| GPUInterfaceQuery.obj | 21 | 2,129 | 1,732 | occlusion queries |
| TexUtil_tfxo_D3D11.obj | 2 | 1,287 | 8 | `GetData_ppfxTexUtil_tfxo` 0x140439ce0: copies the shader container out of the image (no decoding) |
| GPUInterfacePixel.obj | 12 | 1,013 | 220 | pixel containers |
| ppfxInterface.obj | 11 | 336 | 192 | interface stubs |
| ppfxMemoryAllocator.obj | 10 | 291 | 308 | every allocation of the library |
| GPUInterfaceBase.obj | 5 | 179 | 188 | interface bases |
| TexUtil_tfxo_D3D11_0 ... _7.obj (8) | 0 | 0 | 15,135,564 | the shader container, in eight pieces that lie back to back |
| GaussianNoise.obj | 0 | 0 | 262,144 | a 256 x 256 RGBA noise texture |
| LensDisc__*__geometricOptics.obj (6) | 0 | 0 | 196,608 | six lens-disc tables of 32,768 bytes (optical bokeh; whether the game ever uses them: not verified) |

**Totals.** Code: 694,679 bytes = one block of 679,687 bytes with 2,272 functions at
0x14039a650-0x140440a20, plus 12,630 bytes of exception funclets and 2,350 bytes of static initialisers.
Read-only data: 15,792,528 bytes, which is 69 % of the whole `acs.exe`. For comparison: all of Kunos' own
renderer is about 479,000 bytes in 1,162 functions (`docs/atlas/renderer.md`).

The game's own glue is small (`ksCameraYebis.lib`, 54,910 bytes): `CameraForwardYebis.obj` (36 hand-written
functions; one of them, `readPPSetOptions` 0x140026150, is 14,350 bytes of ini reading), `YebisPP.obj` (the
same wrapper for the VR cameras, 20,179 bytes) and `PostProcessEffectsUpdater.obj` (2,102 bytes).

## 3. The frame: call order

Confirmed in the disassembly; the Kunos-side calls are also confirmed by the render oracle, which runs the
game's own `CameraForwardYebis` with YEBIS' start-up and its one per-frame call replaced (section 7).

**Once, when the camera is made** (`CameraForwardYebis::CameraForwardYebis` 0x140025db0, only with
`ENABLED=1`): the shader `ksMSDepthResolve`; with `[POST_PROCESS] FXAA=1` an 8-bit target of the screen's size
(`fxaaTempRT`) and the shader `ksFXAA`; `loadPPSet("default")` 0x140029960 = `readPPSetOptions` (the filter
file, section 5), `initRenderTargets` 0x140029a90 and `initPostProcessing` 0x140024ca0:

- the scene target `yebisRT_MS`: `R16G16B16A16_FLOAT` with a depth buffer, both with `AASAMPLES` samples;
  with more than one sample also a one-sample `R16G16B16A16_FLOAT` target and a one-sample `R32_FLOAT`
  target for the resolved colour and depth (the swap chain and the screen's own depth buffer have one sample
  whenever post-processing is on);
- `IPfxContext_Dx11::Instantiate` 0x14039e5d0; `PFX_INITPARAM::Initialize` 0x1400244e0 (YEBIS is told one
  sample, picks its own buffer formats, `m_uiEffectBufferCriterionWidth = QUALITY / 5 x width`, or
  min(width, 640) at quality 0); `IPfxBaseContext::Initialize(device parameters, glare quality, depth-of-field
  quality, flags, parameters)` 0x14039a700 with the destination's render-target view (the FXAA target or the
  back buffer); `SetDepthFactorRangeFloatOptimizeEnable(true)`; `SetVignetteParameters`;
  `SetApertureAiryDiscEnable` / `...Parameters`.
- Then `PostProcessEffectsUpdater` 0x1400ab880 reads `video.ini [POST_PROCESS] FILTER` and loads that filter
  with the same `loadPPSet`: a second full start of YEBIS when the name is not `default`.
- A change of filter at run time and a window resize destroy and make again all of YEBIS and all targets.

**Every frame**, `CameraForwardYebis::render` 0x14002a450 (blurred node, unblurred node, root, dt):

```
 1  ENABLED off?  -> the plain CameraForward::render, nothing below
 2  shadow maps (three cascades)                              CameraShadowMapped::shadowMapPass 0x14020d6a0
 3  reflection cube map faces                                 CubeMapRenderer::render 0x14021edb0
 4  updateYebisParameters 0x140029e30
      the camera's near / far / field of view, dt, depth of field on and its focus distance, chromatic
      aberration and lens ghosts on, brightness, saturation, the god rays' colour (sun colour by the sun's
      height), the camera and view matrices, where the sun is on the screen
      then: the scene target is cleared (colour 0, depth 1)
 5  evOnPostYebisParametersUpdate                             (only the triple-screen camera listens)
 6  renderApplyEffect 0x14002a740
      the scene target is bound; the scene: CameraShadowMapped::renderPass, or with motion blur
      CameraForward::renderBlurred 0x14021fde0 (blurred branch -> blur target -> solve -> unblurred branch)
      more than one sample: ResolveSubresource (colour), then a full-screen quad with ksMSDepthResolve (depth)
      applyPostProcessing 0x140025c30:
         SetEffectSource(resolved colour, width, height, R16G16B16A16_FLOAT)        0x14039cdd0
         SetDepthFactorSource(depth, ...)                                           0x14039cea0
         setPostProcessing 0x140025080: about 57 setters, in this order:
            scene (SetRenderSceneParameters, camera matrix, view matrix, perspective)
            automatic exposure (enable, [GetAutoExposureAdjusted], delay, range, metering area, middle gray)
            SetTonemapParameters, SetTonemapViewportScale
            heat shimmer (6 calls and the particles)         only with HEAT_SHIMMER=1
            depth of field (5 calls)                         only with DOF > 0
            light shafts (3 calls)                           only with RAYS_OF_GOD=1
            chromatic aberration (2), diaphragm rotation, feedback (3)
            glare (SetGlareShape and 12 tuning calls)        SetGlareShape(0) with GLARE=0
            lens distortion (2), the colour matrix (InitializeMatrix and up to 7 multiplies)
         ApplyEffects(0) 0x14039b230 -> CPostEffect::ApplyEffects 0x1403c5cd0
 7  resetYebisStates 0x14002a650: default device state, the screen bound without a depth buffer
 8  FXAA=1: the FXAA target into slot 5, one full-screen quad with ksFXAA into the back buffer
```

Inside `ApplyEffects` (library code; this is the order of the call sites in
`EndPostEffectScene_InternalProcess` 0x1403b5f40, which is 9,472 bytes; which branch runs under which
setting is **not verified**): save the device's state; resample the scene into the effect buffer; depth
factor; [ambient occlusion: never on]; [YEBIS' motion blur: never on]; heat-shimmer particles; depth of field
and its result blur; [YEBIS' FXAA: never on]; chromatic aberration; lens distortion; glare (source, bloom,
star, ghost, afterimage, light shaft, composite); automatic exposure (a luminance read-back, one frame
late); feedback; the temporal part of the tone map; `CRenderGlare::TonemapToSurface` 0x14041d7f0, the final
pass into the destination (tone map, vignette, then the colour matrix); restore the device's state.

What YEBIS is given: one colour texture (`R16G16B16A16_FLOAT`, one sample, linear HDR values: the engine's
light levels are about three to four times higher with post-processing on, see `HDR_OFF_MULT` in
`docs/port/postfx.md`), one depth texture (`R32_FLOAT`, the hardware depth 0..1), the camera, and its
parameters. Where it draws: the view given at `Initialize`.

Two things the game's own values do **not** do (confirmed): `system/cfg/lighting.ini [HDR] MIN_EXPOSURE /
MAX_EXPOSURE` (`Sim::initHDRLevels` 0x140199b00) are copied into the camera every frame and read by nothing;
the limits YEBIS gets come from the filter file. And the classic HDR chain (`HDR.obj`, `hdr.ini`,
`ksPostToneMap`, `ksHighPass`, `ksPostBlurH/V`) is dead code: no `HDR` object is ever made.

## 4. Where its shaders live

**Embedded in `acs.exe`, as plain compiled Direct3D bytecode.** One container, 15,135,564 bytes, in the
`.rdata` section at 0x1405a3ae0-0x141412e2c (file offset 0x5a26e0 in this build): 2,649 programs (37 vertex
shaders, 2,612 pixel shaders, all shader model 4.0, compiled with the June 2010 HLSL compiler) grouped into
2,653 techniques. No HLSL source, no run-time compile, no compression, no encryption: the library copies the
container (`GetData_ppfxTexUtil_tfxo` 0x140439ce0) and hands a program's bytes straight to
`CreateVertexShader` / `CreatePixelShader` the first time a technique is used
(`CTinyEffect::CreateShaderProgram` 0x14043db70). It never reads a shader from disk, and it imports
`D3DReflect` only (to find a program's constants by name). The three pieces of HLSL source text in `acs.exe`
belong to the font library, not to YEBIS.

The container describes itself (all numbers are decimal text):

| at | size | content |
|---|---:|---|
| 0x00 | 16 | the magic text `.tfxo file` |
| 0x10 | 16 | the length of everything after the first 0x40 bytes |
| 0x20 | 16 | the length of the text index |
| 0x30 | 8 | the number of programs |
| 0x38 | 8 | `hlsl` |
| 0x40 | 314,372 | the text index: the effect's name `TexUtil_D3D11`, 2,653 lines `tech_<name>,<vertex program>,<index>,<pixel program>,<index>`, then 35 global parameters `<name>,<type>,<size>` |
| after it | 2,649 x 256 | one record per program: its file-like name, its offset, its size |
| after that | 14,142,984 | the programs, back to back; each starts with `DXBC` |

Technique families by count: depth of field 832, tone map 707, the multiply-add blur taps about 420, YEBIS'
unused motion blur about 330, bokeh blur 186, bright pass 54, scatter bokeh 44, heat shimmer 16, and a few
each for ghosts, chromatic aberration, lens distortion, light-shaft mask, vignette.

**Can rustyAC load them from the player's own `acs.exe` at run time, like the `.fxo` files? Yes.** The file
on disk is readable (the Steam wrapper leaves `.rdata` as it is in this copy). A loader needs no fixed
offsets and no PDB: find the 16 bytes of the magic in `.rdata` (two hits here: a comparison constant and the
container; the container is the one that has `hlsl` at +0x38), parse the three counts, the technique lines
and the program records, check that every program starts with `DXBC` and that its own length field agrees,
and otherwise give up without drawing. Nothing of it would be copied into the repository or a release.
Not verified: that other builds of `acs.exe` hold the same container (only this one was read).

**But the shaders are the small half.** The container has no pass list, no render states and no constants.
Which techniques run in which order, into which targets, with which blur taps and which numbers, is in the
library's C++ code (section 2).

Other embedded data: the noise texture and the six lens-disc tables of section 2. Everything else is made at
run time (bokeh tables, kernel weights, masks). No texture is read from disk.

## 5. The parameters: `ppfilters/*.ini`, `video.ini` and `PostProcessEffectsUpdater`

Three sources feed one block of values, `KGLPostProcessing` (0x648 bytes, about 190 members, a member of
the camera at +0x828), and `setPostProcessing` 0x140025080 hands that block to YEBIS every frame.

**`cfg/video.ini`** (`loadVideoSettings` 0x1400c18d0; read once; a missing key is 0):

| Key | What it decides |
|---|---|
| `[POST_PROCESS] ENABLED` | everything: which camera is made (`videoSettings.ppHDREnabled`; the name says HDR, it means YEBIS) |
| `QUALITY` (0..5) | the width of YEBIS' effect buffers: `QUALITY / 5 x width`; 0: min(width, 640) |
| `GLARE` (0..5) | glare on (not 0) and its quality: `min(7, 5) x GLARE / 5` |
| `DOF` (0..5) | depth of field on (not 0) and its quality, the same way |
| `RAYS_OF_GOD`, `HEAT_SHIMMER` | light shafts on; heat shimmer on |
| `FXAA` | the game's own FXAA pass after YEBIS |
| `FILTER` | the name of the filter file (read by `PostProcessEffectsUpdater`, not by `loadVideoSettings`) |
| `[VIDEO] AASAMPLES` | the scene target's samples (YEBIS itself always works on one) |
| `[EFFECTS] MOTION_BLUR` | the game's own motion blur before YEBIS |

Whether glare, depth of field, light shafts and heat shimmer are **on** comes only from here. The `ENABLED`
and `QUALITY` lines of those sections in a filter file are read by nothing.

**The filter file** `system/cfg/ppfilters/<name>.ini` (else `Documents/Assetto Corsa/cfg/ppfilters/`),
`CameraForwardYebis::readPPSetOptions` 0x140026150 (the VR cameras have a copy, `YebisPP::readPPSet`
0x14002cd60). Ten files ship: `default`, `default_bright`, `default_dark`, `natural`, `photographic`,
`movie`, `sepia`, `b&w`, `blue_steel`, `vintage`. 117 of their keys are read; where each ends:

| Section | Keys | The YEBIS call they end in |
|---|---|---|
| `[TONEMAPPING]` | `HDR`, `EXPOSURE`, `GAMMA`, `FUNCTION`, `MAPPING_FACTOR`, `SCALE_WIDTH`, `SCALE_HEIGHT`, `OFFSET_X`, `OFFSET_Y` | `SetTonemapParameters(exposure, gamma, function, mapping factor, ...)` 0x14039c1f0 (the exposure is the automatic one when that is on; with `HDR` not 1 the function is fixed), `SetTonemapViewportScale` 0x14039cce0 |
| `[AUTO_EXPOSURE]` | `ENABLED`, `DELAY`, `MIN`, `MAX`, `TARGET`, `INFLUENCED_BY_GLARE` | `SetAutoExposureEnable`, `...Delay`, `...Range(MIN, MAX)`, `...MiddleGray(camera exposure / 30 x TARGET x the exposure multiplier, influenced by glare)` 0x14039beb0 to 0x14039bfd0. The multiplier is `Documents/Assetto Corsa/cfg/exposure.ini [EXPOSURE] VALUE` (the photo mode's and the replay's exposure buttons change it). The camera's exposure is the car's `onBoardExposure` / `dashBoardExposure` / `outBoardExposure` (30 for the track cameras) |
| `[COLOR]` | `HUE`, `SATURATION`, `BRIGHTNESS`, `CONTRAST`, `SEPIA`, `COLOR_TEMP`, `WHITE_BALANCE` | `InitializeMatrix`, then one `MultiplyMatrixColor...` each when it is not the neutral value (hue in degrees to radians; 6500 K is neutral) 0x14039d780 to 0x14039dad0. Saturation is multiplied by `video.ini [SATURATION] LEVEL` / 100 |
| `[GLARE]` | `SHAPE`, `GHOST`, `GHOST_CONCENTRIC_DISTORTION`, `AFTER_IMAGE`, `ANAMORPHIC`, `LUMINANCE`, `THRESHOLD`, `BRIGHT_PASS`, `PRECISION`, `BLUR`, `BLOOM_*` (4), `GENERATION_RANGE_SCALE`, `STAR_*` (3), `USE_CUSTOM_SHAPE` and 19 `SHAPE_*` keys | `SetGlareShape` (a preset number, or a preset's definition with its luminances and lengths multiplied by the `SHAPE_*` values), `SetGlareParameters`, and ten tuning setters 0x14039b550 to 0x14039d4c0 (`STAR_LENGTH_FOV_DEPENDENCE` is read and then lost: the setter is fed a member nothing writes, always 0); `GHOST`, `AFTER_IMAGE`, `PRECISION` and `ANAMORPHIC` go into `Initialize` (a change restarts YEBIS) |
| `[DOF]` | `APERTURE_PARAMETER`, `APERTURE_F_NUMBER`, `IMAGE_SENSOR_HEIGHT`, `BASE_FOV`, `ADAPTIVE_APERTURE_FACTOR`, `APERTURE_FRONT_LEVELS_NUMBER`, `APERTURE_BACK_LEVELS_NUMBER`, `EDGE_QUALITY`, `BACKGROUND_MASK_THRESHOLD`, `APERTURE_SHAPE` | `SetDepthOfFieldParameters(focus distance, f-number, ...)` 0x14039b6d0 and three more; the focus distance and whether it is active come from the game's camera of the moment (`Camera::dofFocus`, `dofFactor`) |
| `[GODRAYS]` | `USE_SUN_LIGHT`, `LENGTH`, `GLARE_RATIO`, `ANGLE_ATTENUATION`, `NOISE_MASK`, `NOISE_FREQUENCY`, `DEPTH_MASK_THRESHOLD`, `DIFFRACTION_RING` and four more of it | `SetLightShaftParameters`, `SetLightShaftDiffractionRing`, `SetLightShaftPosition` 0x14039b630 to 0x14039b670; the position is the sun on the screen, the colour the sun's (both computed every frame by `updateYebisParameters`), and it is off when the sun is behind the camera |
| `[HEAT_SHIMMER]` | 14 `HEAT_PARTICLE_*` keys | `SetHeatShimmerParticleParameters`, `...FractalOctave`, `...FalloffDistance`, `...Color`, `...Array` 0x14039ca40 to 0x14039cb60; the particles themselves are the game's (`KGLYebisParticle*`, `rand()`), one volume 2 m behind each car, switched off above 25 km/h (`PostProcessEffectsUpdater::update` 0x1400abc20) |
| `[CHROMATIC_ABERRATION]` | `ENABLED`, `SAMPLES`, `LATERAL_DISPERSION`, `UNIFORM_DISPERSION` | `SetChromaticAberrationEnable`, `...Parameters` 0x14039c5f0 |
| `[VIGNETTING]` | `STRENGTH`, `FOV_DEPENDENCE` | `SetVignetteParameters` 0x14039c440, at start-up only |
| `[AIRYDISC]`, `[DIAPHRAGM]`, `[FEEDBACK]`, `[LENSDISTORTION]`, `[VARIOUS]`, `[DEBUG]` | `ENABLED`, `WAVELENGTH`, ...; `ROTATE_SCALE`, `ROTATE_OFFSET`, `TYPE`; `ENABLED`, `ASPECT_RATIO`, `WEIGHT`, `CURRENT_WEIGHT`, `TIME`; `ENABLED`, `ROUNDNESS`, `SMOOTHNESS`; `APERTURE_RESULT_BLUR_MOD`; `DOF_FOCUS` | `SetApertureAiryDisc...` (start-up), `SetDiaphragmRotate`, `SetFeedback...`, `SetLensDistortion...` |

In the files and read by nothing: `[YEBIS] ENABLED`, `[OPTIMIZATIONS] FIXED_WIDTH`, every `ENABLED` and
`QUALITY` of `[DOF]`, `[GLARE]`, `[GODRAYS]`, `[HEAT_SHIMMER]`, all of `[ANTIALIAS]`, and the four
`[AUTO_EXPOSURE] METERING_*` keys (the metering area is fixed: the whole picture).

**`PostProcessEffectsUpdater`** (0x1400ab880, a game object made with the camera; 2,102 bytes) does two
things and never calls the library: it loads the filter named in `video.ini [POST_PROCESS] FILTER` (and
again when the filter app `PostProcessFilterSelector` picks another: `loadConfigurationFile` 0x1400abb70 ->
`loadPPSet`), and every frame it fills the heat-shimmer volumes behind the cars. How the cars' state would
reach other parameters was an open question of the atlas: it does not; exposure comes from the camera
definition, focus from the camera of the moment.

Traps for a port (all confirmed): a key that is missing from a section reads as 0, not as a default; a
section that is missing keeps the values of the filter loaded before; text that is not a number ends the
game; `sepia.ini` and `vintage.ini` say `SHAPE_GHOST_HALO_LUMINANCE=05`, which is 5; three angles are
turned into radians with a single-precision constant and one multiply.

With this PC's `video.ini` of the moment and only `ENABLED` switched to 1 (`QUALITY`, `GLARE`, `DOF`,
`RAYS_OF_GOD`, `HEAT_SHIMMER` all 0), YEBIS would do: tone map, automatic exposure, the colour matrix,
chromatic aberration and the vignette of `default.ini`, in effect buffers 640 wide. That is stage 3 of the
plan below.

The exact conversion of every key (defaults, units, which member of the block) is in the brief
`re/scratch/task23/yebis_3_params.md` and in `yebis_2_frame.md` section 9.

## 6. A port plan for Task 23b

**What a port needs, in the order that can be proven piece by piece:**

1. **The parameter block** (`KGLPostProcessing`, 0x648 bytes, about 190 values): `readPPSetOptions`
   (14,350 bytes, about 190 ini keys, mechanical), `updateYebisParameters` (1,565 bytes: the sun on the
   screen, the god rays' colour), `PostProcessEffectsUpdater` (2,102 bytes), the missing `video.ini` keys.
   Proof: the oracle reads the game's block (the camera object at +0x828) after each frame and compares it
   byte for byte with the port's. About 20 kB of original code. **Half a task.**
2. **The shader container's loader** and the library's device layer as far as it shows in the command log
   (its state cache decides which state calls reach Direct3D). **Half a task.**
3. **The tone-map path**: `ApplyEffects` with glare, depth of field, light shafts and heat shimmer off, which
   is a setting the game itself offers (`[POST_PROCESS] GLARE=0, DOF=0, RAYS_OF_GOD=0, HEAT_SHIMMER=0`):
   resample, automatic exposure (the luminance read-back), the tone map with its vignette, the colour matrix,
   chromatic aberration, feedback. This is what brings the engine's HDR levels back into range, so it is the
   part that makes post-processing usable at all. Proof: command log and WARP pixels of whole frames against
   the game's real YEBIS (the oracle already hosts `CameraForwardYebis`; the two patches of section 7 come
   out). **One to two tasks.**
4. **Glare** (bloom, star, ghost, anamorphic, afterimage) and **light shafts**: `CRenderGlare`, 47 kB, plus
   the blur toolbox. **One to two tasks.**
5. **Depth of field**: `CDepthOfField`, 81 kB, the largest single piece, with 832 techniques to choose from.
   **One to two tasks.**
6. **Heat shimmer** (its particles use `rand()`), **lens distortion**, the rest. **Half a task.**

**Estimate: five to seven tasks of the size of Tasks 20 to 22c (four hours each), 20 to 28 hours.** That is
for the slice the game uses, perhaps 300 of the 680 kB; the unused motion blur, ambient occlusion, auto
focus and optical bokeh stay out.

What makes it harder than Kunos' renderer was: no type information at all (function names only); a deep
call graph of small helpers (2,272 functions); the library uses the C runtime's `powf`, `expf`, `logf` and
vector versions of them, whose results a 1:1 port has to reproduce (`rustyac-math` has the scalar ones); the
automatic exposure reads a texture back from the card, which may arrive a frame later on a real card than on
WARP (to be looked at first).
What makes it easier: everything it does goes through the device context, so the command log shows every
state, every shader (by hash), every constant buffer (every byte) and every target; a wrong pass is found at
its first call. The shaders need no translation. And the method is the one every renderer task has used.

**The alternative, a look-alike:** an own tone mapper (the filter's function and gamma), an own bloom and
depth of field, fed by the same parameter block. One to two tasks to something that looks plausible. It
could never be held against the game: every filter would be "about right", and the 190 parameters would be
honoured as well as the imitation manages. It would also be rustyAC's first picture that is not Assetto
Corsa's.

**Recommendation: port it 1:1, in the stages above, and prove each with the oracle.** Stage 3 alone already
gives the correct picture for a setting the game offers (post-processing on with glare and depth of field
off), so there is no need for an imitation in between; until then rustyAC draws the post-processing frame
with the light levels of post-processing off (`docs/port/postfx.md`). If Stage 3 shows that the read-back
or the maths cannot be made exact, the question can be asked again with facts; a look-alike can still be
written then, and steps 1 and 2 are needed for it anyway.

## 7. What Task 23 left ready for it

- `crates/rustyac-render/src/post.rs`: `CameraForwardYebis` without YEBIS (targets, the frame, the resolve,
  the depth resolve, the motion blur's hook, the FXAA pass). Where the game calls `applyPostProcessing` it
  draws a plain copy with the game's `ksPostCopy` shader.
- `tools/render_oracle --post`: the game's own `CameraForwardYebis` in the oracle, with two patches:
  `initPostProcessing` 0x140024ca0 returns at once (the context pointer stays null, and every YEBIS call of
  the game tests it), and `applyPostProcessing` 0x140025c30 jumps to the same plain copy. Task 23b takes the
  two patches out and compares against the real thing.
- The game's side of the oracle runs every frame at 1/60 s and on WARP, so YEBIS' time-dependent parts
  (exposure delay, feedback, afterimage) get the same inputs on both sides.
