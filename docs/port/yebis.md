# Task Y5: YEBIS's glare and light shafts (and Y3e's leftovers) (v0.49.0, in progress)

## Resume here

- **Step 0 done** (Y3e-b's leftovers): steps 0.1, 0.2, 0.3 committed. Next: step 1 (glare).
- Decision (2026-11-12): YEBIS stays desktop only; Task Y4 (the page) is dropped (plan table
  below, README's browser row, the page's settings say "(desktop only)").
- Step 0.1: live `rustyac.exe` opens the headset first and makes its `CameraForwardYebis` with
  `stereo` = a headset opened, as `Sim::createCamera` 0x1401982e0 (`initOculus`/`initOpenVR`
  before the camera; on failure MODE=DEFAULT and the flat YEBIS camera). The `shortcut:` in
  `crates/rustyac-game/src/render/ac.rs` is gone. Checked live: OpenVR stand-in root
  (`re/scratch/taskY5/vr01`, eyes drawn, post-processing on, only eye-size targets bound) and
  the real root without a runtime (`vr02`, note "VR: Could not initialize the vive; the plain
  camera", flat YEBIS).
- Step 0.2: `render_list.txt` rows take `RESIZE:<frame>:<W>x<H>` (`render.sh` sets
  `RENDER_ORACLE_RESIZE`); the old resize scripts are rows `ry5_01` (FXAA, Y3), `ry5_02`
  (triple, Y3d-2), `ry5_03` (first shimmer after a remake, Y3e step 0.4).
- Step 0.3: resize in VR with post-processing, rows `ry5_04` (`--vr --post`) and `ry5_05`
  (`--oculus --post --fxaa`). The first run found a real difference (pixels IDENTICAL, call log
  not on the resize frame): `CameraForwardYebis::on_resize` remade the flat YEBIS targets in VR,
  which the game, having no `CameraForwardYebis` there, never makes. Fixed in
  `crates/rustyac-render/src/post.rs` (`stereo()`); now `ry5_01`..`05` 5 of 5 IDENTICAL
  (`re/scratch/taskY5/proof0.txt`, `proof1.txt`).
- Check step 0: `RENDER_ONLY='ry5_0[1-5]' PROOF_OUT=re/scratch/taskY5/proof sh tools/proofs/render.sh`.

# Task Y3e: YEBIS in VR, and the Y3d-3 shortcuts (v0.48.8)

## Resume here

- **Done (v0.48.8).** Steps 0, 1 and 2: the Y3d-3 shortcuts are gone, VR runs YEBIS on OpenVR
  and Oculus, proven on the stand-ins. Next: Task Y4 (YEBIS in the page); leftovers go to Task
  Y3e-b (below).
- End checks: `render.sh` rows `r23_09`..`r23_24`, `r47b_01`..`08`, `r47c_01`..`05`, `ry3e_01`..`05`
  34 of 34 IDENTICAL (`re/scratch/taskY3e/proof2.txt`); `run_all.sh --quick` 170 of 170
  IDENTICAL (`quick.log`); clippy native `--all-targets`, both wasm targets and wgpu clean
  (`re/scratch/taskY3e/clippy.sh`; render_oracle's own 42 old clippy errors unchanged).
- Task Y3e-b (left): live `rustyac.exe` still makes the flat camera's YEBIS targets beside a
  headset's (`shortcut:` in `crates/rustyac-game/src/render/ac.rs`); the YEBIS remake on a
  resize in VR is not proven (`RENDER_ORACLE_RESIZE` with `--vr --post` not run); the
  first-shimmer-after-remake case is a script (`heat.sh`), not a row, because rows cannot set
  `RENDER_ORACLE_RESIZE`.
- New files: none in the repo's code (the web zip gains nothing); scratch scripts in
  `re/scratch/taskY3e`.
- Check step 0.1: `bash re/scratch/taskY3e/rand2.sh` (game side, 2 cars, `RENDER_ORACLE_RAND_LOG=1`)
  and `cargo test --release -p rustyac-render --lib heat_shimmer`.
- Check step 0.2: `LABEL=s2 bash re/scratch/taskY3e/cmp.sh` (the Y3d-3 compare, 52 frames,
  writes `re/scratch/taskY3e/s2.log`); the game's sampler modes: `bash re/scratch/taskY3e/samp.sh`
  (`RENDER_ORACLE_SAMPLER_LOG=1`, `SSM` lines in `samp.log`).
- Check step 0.3: `LABEL=h1 bash re/scratch/taskY3e/heat.sh` or the row:
  `RENDER_ONLY=ry3e_01 PROOF_OUT=re/scratch/taskY3e/proof sh tools/proofs/render.sh`.
- Check step 0.4: `RENDER_ORACLE_RESIZE=48:1024x576 LABEL=r1 FRAMES=56 DUMP=49,55 bash
  re/scratch/taskY3e/heat.sh` (dump only frames after the resize).
- Check step 1: `MODE=--vr LABEL=v3 FRAMES=20 bash re/scratch/taskY3e/vrcmp.sh` and
  `MODE=--oculus LABEL=o1 bash re/scratch/taskY3e/vrcmp.sh` (`EXTRA="--aa 4"` or `"--fxaa"` for
  the other two paths; the game's side alone: `re/scratch/taskY3e/vrgame.sh`).

## Step 1: YEBIS in VR (`YebisPP`, both headsets)

- The game: with video.ini `ppHDREnabled` the stereo camera's constructor
  (`StereoCameraVive` 0x140220b80, `StereoCameraForward` 0x140211370) turns `ppHeatShimmer`
  off, makes one `YebisPP` 0x14002ba30 for both eyes (the eye's size, the left eye's texture as
  its frame buffer), `loadPPSet("default")` (never video.ini's FILTER), turns chromatic
  aberration off (+0x4b4), and makes `rtYebisTarget` (multisampled with AA, then
  `rtYebisResolved` and `rtYebisResolvedDepth`) and, with FXAA and no AA, `rtFXAA_Temp` per eye.
  `createCamera` makes no `CameraForwardYebis` and no `PostProcessEffectsUpdater` in VR.
- `initPP` 0x140030540 computes the base width from QUALITY and then overwrites it with the
  eye's width: `Yebis::new` now takes the base width, the callers compute it.
- `renderYebis` (0x140223f60, 0x140213b10) per eye: the eye into `rtYebisTarget`,
  `updatePPCameraValues` 0x14002bcc0 (chromatic aberration off, no depth of field),
  `updatePP(eye == 0)` 0x14002c270, the MS resolve and depth resolve, YEBIS into the eye's
  texture or into `rtFXAA_Temp` then FXAA into the eye; Oculus asks `ovr_GetHmdDesc` first and
  commits each eye's chain after its YEBIS.
- Two things the flat camera never shows:
  - Without chromatic aberration YEBIS skips its two CA draws
    (`DrawRectGPU_ChromaticAberration` 0x1403f0120; the Y2 notes called draw 1 camera motion
    blur) and the luminance and tonemaps read the scene directly: `Camera::chromatic_aberration`.
  - `updatePP` sets the tonemap exposure (`SetTonemapParameters`, CPostEffect +0x6f4) only for
    the first eye, so the second eye's auto exposure works from the first eye's value:
    `Camera::new_exposure` and `AutoExposure::tonemap`. Without it frame 0's second eye differs
    (cbuffer +0x9a0 1.0 against 0.5).
- The oracle's game side makes the stereo camera instead of `CameraForwardYebis` when VR and
  `--post` meet (it crashed in `finishRendering` before), and needs `RENDER_ORACLE_YEBIS=1`.
- Live `rustyac.exe` keeps making the flat camera's targets (a headset that fails falls back to
  the screen), marked `shortcut:` in `render/ac.rs`.

| Case (`vrcmp.sh`, Spa dash, trk_lap, 1280x720) | Frames | Call log | Pixels | Runtime calls |
|---|---|---|---|---|
| `--vr --post` (v3) | 20 | IDENTICAL | IDENTICAL | 390 the same |
| `--vr --post --aa 4` (v4) | 8 | IDENTICAL | IDENTICAL | 174 the same |
| `--vr --post --fxaa` (v5) | 8 | IDENTICAL | IDENTICAL | 174 the same |
| `--oculus --post` (o1) | 8 | IDENTICAL | IDENTICAL | 168 the same |
| `--oculus --post --aa 4` (o2) | 8 | IDENTICAL | IDENTICAL | 168 the same |
| `--oculus --post --fxaa` (o3) | 8 | IDENTICAL | IDENTICAL | 168 the same |
| plant: both eyes take a new exposure, `--vr` (vp) | 4 | frame 0 DIFFERENT, 8 lines | 460,685 of 1,843,200, max 85 | the same |
| the same plant, `--oculus` (op) | 4 | frame 0 DIFFERENT, 8 lines | 460,632 of 1,843,200, max 85 | the same |

## Step 0.4: the first shimmer after a YEBIS remake (proof only, no change)

- The 100-times filter, the window resized to 1024x576 before frame 48 (three frames into the
  shimmer): call log and pixels IDENTICAL in all 55 frames (`r1`).
- Planted change: the remade YEBIS keeps the old copy sampler (point/clamp) instead of starting
  again at linear/border (`r1p`): frame 48's call log DIFFERENT (1 line, at line 7448), pixels
  the same there (the copy is texel to texel). So the game starts the copy's mode again with the
  new textures, as the port's `Yebis::remade` does.
- Not a render.sh row: `render_list.txt` rows cannot set `RENDER_ORACLE_RESIZE`.

## Step 0.3: a shimmer row that moves pixels (`ry3e_01`)

- No fake install was needed: both sides of `render_oracle` read the filters from the oracle's
  own scratch root (`root::prepare` copies the game's `system/cfg/ppfilters` there), never from
  the install.
- `--ppfilter rustyac_heat_x100`: `prepare` writes it into the root from the root's `default.ini`
  with `HEAT_PARTICLE_SHIMMER=0.4` (100 times 0.004), and it turns `[POST_PROCESS] HEAT_SHIMMER`
  on. Nothing of AC's goes into the repo.
- New full row `ry3e_01` (not quick): `spa_launch`, 52 frames, `--post --fxaa`, the shimmer from
  frame 45. IDENTICAL in `render.sh`.
- `re/scratch/taskY3e/heat.sh` (`h1`): call log and pixels IDENTICAL in all 51 frames. The same run
  with `default.ini` (`h0`, IDENTICAL too): the game's frame 50 differs from `h1` in 86,603 of
  921,600 pixels, frame 44 (before the shimmer) in none.
- Planted change (port's shimmer 1.25 times, `h1p`, 48 frames): frames 45 to 47 DIFFERENT, call
  log 4 lines, pixels 83,029 of 921,600, largest channel difference 200.

## Step 0.2: the copy draw's sampler (the `copy_sampler` shortcut is gone)

- New hook `RENDER_ORACLE_SAMPLER_LOG=1`: every `IGPUBaseTexture::SetSamplerStateMode`
  0x140436580 (its `_Filter` and `_Address` tail-call it) as `SSM texture old new` and three
  return addresses. Mode = address * 4 + filter (filter 0 linear, 1 point; address 0 border, 1
  clamp, 3 wrap).
- `DrawRectGPU_TonemapHDR` 0x1403f1b10 saves its two textures' modes, sets filter and clamp, and
  at the end (machine code at 0x1403f29bd to 0x1403f29d1) restores the first texture's mode, then
  stores the **second** texture's saved mode into the **first** again. The second texture is
  the black one (`CRenderGlare::TonemapToSurface` 0x14041d7f0), mode 5 (point, clamp) from the
  start. So every tonemap leaves its source at point/clamp.
- In a shimmer frame the tonemap's source is the same texture object the shimmer copies from
  (game log, frame 45: copy at mode 0, tonemap 0 to 5, the next copy at 5). Before frame 45 the
  tonemap reads another texture and the copy source stays at mode 0.
- The port now switches `copy_sampler` to point/clamp after round one's tonemap in a shimmer
  frame, not in `shimmer()`.
- Proof: call log and pixels IDENTICAL in all 51 frames (`s2`). Planted change (no switch):
  call log DIFFERENT in frames 45 to 47, first at line 5472; pixels IDENTICAL (a same-size copy
  gives the same pixels with either sampler).

## Step 0.1: the shimmer sets are made once, from race.ini's cars

- acs.exe: `Sim::createCamera` 0x1401982e0 reads race.ini `[RACE] CARS` and passes it to
  `CameraForwardYebis::CameraForwardYebis`, which makes that many particle sets once.
- rustyac.exe made 1 set at start, then remade them in `set_cars_to_be_loaded`. That drew 60
  `rand()` more per extra car than the game.
- Now `AcOptions::cars` carries the count into `start_yebis`, and the remake (`set_heat_sets`,
  `Yebis::set_particles`) is gone. `render_oracle`'s port side already did this.
- Proof (`re/scratch/taskY3e/rand2.sh`, 2 cars): between `srand(22)` and `srand(21)` the game
  calls `rand()` 120 times, all from `KGLYebisParticle::initialize` 0x140024600 (return
  140024867). Their values equal the MSVC stream seeded 22, the stream `remake` draws from. The
  test `two_cars_take_120_rands_after_seed_22` checks that 2 sets take exactly those 120.

# Task Y3d-3: YEBIS's heat shimmer (v0.48.7)

## Resume here

- **Done, released as v0.48.7.** With video.ini `[POST_PROCESS] HEAT_SHIMMER=1`, `rustyac.exe`
  and `render_oracle`'s port side draw the shimmer behind slow cars (under 25 km/h) as the game
  does: call log and pixels IDENTICAL, 1 car and 2 cars. Next is **Task Y3d-3b** (leftovers
  below), then Y3e (VR with YEBIS).
- Check the shimmer: `LABEL=c2 bash re/scratch/taskY3d3/cmp.sh` (52 frames of `spa_launch`,
  `RENDER_ORACLE_HEAT_SHIMMER=1 RENDER_ORACLE_YEBIS=1`, dumps 44/46/50; the shimmer shows from
  frame 45). The 2-car row: `LABEL=c3 FRAMES=100 DUMP=50,99 CARS="--tapes
  oracle/t24b/field/mc_field --max-cars 2" bash re/scratch/taskY3d3/cmp.sh`.
- `bash re/scratch/taskY3d3/end.sh` is the whole step 3: planted change, both rows again, the 16
  `--post` rows, `run_all.sh --quick` and clippy every way; it writes `end.txt`.
- Particles only: `bash re/scratch/taskY3d3/heat2.sh` (`RENDER_ORACLE_HEAT_DUMP=1`, the per-frame
  `HEAT` lines of both sides, SAME).

## Results

| Case | Frames | Call log | Pixels |
|---|---|---|---|
| `spa_launch`, 1 car, f2004, 1280x720, `--post --fxaa`, HEAT_SHIMMER=1 (`f1`) | 51 | IDENTICAL | IDENTICAL |
| `mc_field`, 2 cars, same options (`f2`) | 99 | IDENTICAL | IDENTICAL |
| Planted: the sort turned nearest first (`plant`) | 51 | DIFFERENT in 7 frames from 45 (caught) | IDENTICAL |
| The 16 `--post` rows (r23_09..r23_24) | 1 each | IDENTICAL | IDENTICAL |
| `run_all.sh --quick` | 170 cases | 170 IDENTICAL | |

The planted sort changes the vertex buffer's bytes, which the call log catches; the pixels stay
the same because at the default filter's strength the shimmer changes no pixel here at all:
`rustyac.exe --warp` at Spa's start (chase view, 10 particles drawn) gives the same PNG with
HEAT_SHIMMER on and off. A temporary build with the vertex weight times 100 shows the shimmer
behind the car (re/scratch/taskY3d3/shimmer_x100_probe.jpg). So at `default.ini` (SHIMMER=0.004)
the shimmer is likely below one 8-bit step in the oracle's frames too (not measured there: no
game run with the shimmer off on the same tape). A proof row that moves pixels needs a stronger
filter.

## Step 2 part 2: YEBIS's draws (done)

- `crates/rustyac-render/src/yebis_heat.rs` (new), in the disassembly's order:
  - `setupPfxParticles` 0x140024bb0 (the 14-float `PFX_PARTICLE_HEATSHIMMER`),
  - the view YEBIS keeps at `CPostEffect`+0x59c: **`SetRenderSceneViewMatrix` 0x1403c2190 runs
    after `SetRenderSceneCameraMatrix` and overwrites it**: inverse(ChangeHandedness(inverse(
    `Camera::getViewMatrix`))). Built from the camera's world matrix instead, the yaw/pitch/roll
    were off by ulps,
  - the frustum of `UpdateParameters_EffectiveHeatShimmerParticleEnable` 0x1403ba2e0,
  - `SetupHeatShimmerParticles` 0x1403fd860 (view-space cull, then `qsort` farthest first;
    shortcut: a stable sort, msvcr120's qsort could order equal depths differently),
  - `DrawIndicesGPU_HeatShimmerParticles` 0x1403fc950: the variables at 800, 2464, 3360 and 3584
    and the 44-byte vertices (4 per particle).
- `yebis.rs`: three effect surfaces (the third is the shimmer's copy), the programs
  `HeatShimmerParticle_S1..S4` with packed float3/float4 layouts, three vertex buffers of
  `m_uiMaxParticles` quads used in turn, and `HeatShimmerParticles` 0x1403fdc70 after each round's
  first draw: the MadT1 copy, then the particles; the round's second draw reads the copy.
- The copy samples with its source's own sampler mode. It is linear/border on the first shimmer
  and point/clamp after (game logs f45/f46); shortcut: the port switches after its first copy,
  the draw that changes the source's mode in the game is not traced (`DrawRectGPU_TonemapHDR`
  0x1403f1b10 restores both textures' modes into the first one, a candidate).
- `rustyac.exe`: `set_cars_to_be_loaded` remakes the sets and YEBIS's shimmer buffers for the
  car count (`CameraForwardYebis::set_heat_sets`).

## Step 2 part 1: the particles (done)

- `crates/rustyac-render/src/heat_shimmer.rs` (new): the [HEAT_SHIMMER] keys, the particle sets,
  `KGLYebisParticle::initialize` 0x140024600, `PostProcessEffectsUpdater::update` 0x1400abc20
  (off above 25 km/h; each car's volume 2 m behind its body) and `KGLYebisParticleSet::update`
  0x140024950. Random numbers come from the graphics `crt_rand`, as the game's render thread.
- rustyac.exe: one set at start, remade with the real car count in `set_cars_to_be_loaded`
  (shortcut: acs.exe makes CARS sets once in `Sim::createCamera`; the remake draws more `rand()`).
- The oracle's game side passes `1 + others` sets to the `CameraForwardYebis` constructor.

## Step 0 (done)

- 848d613: the s1024 flake fix (CRenderGlare's glare scale is zeroed in its constructor).
- 28dd5a3: the 38 fixed teardown releases, each named by its owner.
- b7ed687: `RENDER_ORACLE_PP_QUALITY` and `RENDER_ORACLE_HEAT_SHIMMER` set video.ini and the
  game's VideoSettings alike.
  - Before this, the oracle's hand-made VideoSettings hard-coded `ppQuality` at +0x3c to 0.
  - It now writes `ppHeatShimmer` at +0x39 too.
- 4c49ebb: YEBIS's base width follows the camera's resized width, not the FXAA target's.
  - `CameraForwardYebis::onResize` 0x140029da0 sets `width`, then `initRenderTargets` 0x140029a90
    runs again.
  - `quality.sh`: QUALITY 5 and 3, resized at frame 10, are IDENTICAL in all 13 frames, by call
    log and by pixels. Before the fix, frame 10 differed in 159 log lines.

## Step 1: rand() on the render thread (measured with `RENDER_ORACLE_RAND_LOG=1`)

- At start, in this order:
  1. `CameraFacing` calls `srand(0)` at 14005e737.
  2. About 36,700 crowd `rand()` calls follow.
  3. `srand(0)` at 14005f78a.
  4. `ksRandomize(22)`.
  5. **60 `rand()`: `KGLYebisParticle::initialize` 0x140024600, called from the set
     constructor 0x1400247f0 (returns to 140024867).** That is 10 particles × 6.
  6. `srand(21)` for the car.
  7. One `ksRand` call at 14003378a (from 1400e024c).
  8. Track load adds 9 more `rand()` calls at 1401c9b7e…1401ca20f.
- `initialize` makes **6** `rand()` calls, not 7. They are at 140024681, 1400246b6, 1400246e1,
  14002470e, 140024751 and 14002477c.
- No `rand()` runs in frames 0–5. A particle calls `initialize` again when its life (0.75 to
  1.75 s) runs out, so the frames from about 45 on draw from the same stream.
- So the port must:
  - take the 60 calls from `graphics.crt_rand` after the track seed (22) and before the car
    seed (21);
  - re-initialise from that same stream inside the frame loop.
- At start, the volumes are still zero. Particle 0 starts at (0,0,0) with radius 0. Its first
  re-initialise puts it at the car: the volume is about (-188.39, 5.66, -253.17), 1.5 × 1.5 × 0.

## Not done (Y3d-3b)

- The shimmer's blend program (`+0xa8c`, `SetHeatShimmerParticleBlend`): AC never calls it, not
  ported.
- The copy's sampler switch is a shortcut (above); the first shimmer after a YEBIS remake (resize)
  is not proven.
- Tied particle depths: the port's stable sort against msvcr120's qsort, not seen in the rows.
- `rustyac.exe` makes one set at start and remakes the sets for the car count (more `rand()` than
  acs.exe, which makes them once in `Sim::createCamera`).
- No shimmer row in `tools/proofs` (the proof is `cmp.sh`), and no row with pixels the shimmer
  changes (a ppfilter with a larger SHIMMER, needing a copied game folder).

## Open questions

- Which game draw leaves the effect surface's sampler mode at point/clamp after the first shimmer?

# Task Y3d-2b: YEBIS's sizes from the game's code, and its two flakes (v0.48.6)

## Resume here (Y3d-2b)

- **Released as v0.48.6.** The four fitted values are now rules read from the game's code. Both
  flakes had one cause, a heap byte the game never sets, and the oracle now zeroes it.
- **Next: Y3d-2c** (leftovers, see "Not done"). **Heat shimmer stays in Y3d-3.**
- Check:
  - `bash re/scratch/taskY3d2b/sizes.sh` runs four never-measured sizes, each resized at frame 10
    (14 frames each).
  - `bash re/scratch/taskY3d2b/end.sh` runs trirsz20 five times, then the 16 `--post` rows, then
    `run_all.sh --quick`.
  - `RENDER_ORACLE_RT_LOG=1` now also prints `RTA` lines (each `CRenderTexturePool::AllocateTextures`
    call: the request, its two callers, what it returned, and the pool's list) and `RTD` lines (each
    `CRenderTexture::InitializeDevice`, with six return addresses).

## The sizes, from the code

All sizes hang off the effect buffers' base width. `CameraForwardYebis::initRenderTargets`
(0x140029a90, decomp :122-131) sets `optFixedWidth = (int)(ppQuality / 5.0f * width)`, or
`min(640, width)` when that is 0. `ppQuality` is video.ini `[POST_PROCESS] QUALITY`.
`initPostProcessing` hands it to YEBIS as the effect-buffer criterion width, and
`CPostEffect::Initialize` uses that as its base width (:764-771). This is `base_width`.

- **Glare** (`glare_size`). `CPostEffect::Initialize` :1515-1530 computes
  `(u32)(base * factor + 0.99)`, clamped to the base and the frame width. The height is
  `(u32)(w / aspect + 0.499999)`. Below 16 the min-16 rule applies. The factor is
  table 0x141521650[glare quality]: 0.125 at GLARE=0.
- **DOF level 0** (`dof_sizes[0]`). Same shape, :1647-1700. The factor is
  table 0x141521690 + quality·0x1c: 0.25 at DOF=0. 1280x720 gives 160x90.
- **DOF level 1** (`dof_sizes[1]`). `CDepthOfField::CreateApertureFilterLevelParams` 0x14041f750
  picks factor index 1 (table 0x14052dce8 = 0, 0.5, 0.75, 1) only when `this[0x129] == 0` and
  `w0 <= this+0x118 || w0 <= this+0x144 · this+0x11c`. Those two are the DOF table's +0x14 (128) and
  +0x18 (0.125), copied by `CDepthOfField::Initialize`; +0x144 is the frame width. Then level 1 is
  (min(2w, W), min(2h, H)), else level 0's size again. So at 1280x720 (160 ≤ 160) it doubles, at
  1024x770 (160 > 128 and > 128) it does not: each level's own target is 160x120.
- **DOF halvings** (`half_pool`). `CDepthOfField::CreateSystemObjects` :185-205 and :1284-1294:
  `n = max(0, ceil(ln(max((d0w+1)/W, (d0h+1)/H)) / ln 0.5) - 1)` halvings of W×H, each
  `(u32)(x * 0.5 + 0.99)`. They come from the shared pool, so a size the pool already holds is
  shared. The pool order is the DOF level-0 entry, then its double, then the halvings it lacks (RTA
  log).
  - The level asks the shared pool for that pair only when it doubles. `DOFLEVELPARAM::Initialize`
    0x14041e420 counts +0x60 up only when the two sizes differ, and the level loop
    (CreateSystemObjects :816-858) allocates nothing when +0x60 and +0x70 are 0. So at 1024x770 the
    shared pool holds only the halvings (512x385, 256x193).
- **Clear order** (`half_pairs`). `CDepthOfField::ClearAllGPUResources` 0x14042b930 clears:
  - first its own pool (level 0);
  - then the halvings list (this+0x458, count +0x460);
  - then, in the level loop, level 1.
  The RT_LOG `RTS` callers confirm it: 14042b954, 14042bb63, 14042bbb2.
- **Teardown count.** `CRenderTexture::UninitializeDevice` 0x1403d29f0 puts the screen back only when
  the texture holds a GPU texture (+0x28). So the count is one per made texture:
  - 38 at every size;
  - plus the glare's and DOF's pools;
  - plus the luminance chain.
- **The aspect after a resize.** `CPostEffect+0x2fc` is set once in `Initialize` (:784-788), and the
  frame-10 remake is made at the old size. The chromatic aberration's matrix uses it. The port took
  the aspect from the current frame size; `Yebis::render` now takes YEBIS's made size.
- The old fitted `dof_sizes` (round(320·H/W)) gave the same sizes at every measured size. It is off by
  one when frac(160·H/W) is in [0.25, 0.5), for example 1024x770 (old 160x121, code 160x120).

## The run-dependent extra target and the trirsz20 flake (one cause)

- `CPostEffect` is `CPfxMemoryAllocator::Alloc(0xbd8)`. Its constructor (0x1403bb820) never writes the
  ambient-occlusion flag at +0x131. `Initialize` only sets it (:529-531). When that heap byte is
  non-zero, `Initialize` makes `m_pAmbientOcclusion_NormalDepthTexture` (:1841-1853): an extra W×H
  R16G16B16A16, at start-up and in every remake. The teardown frees it too, which accounts for the
  extra pair.
  - Proof: the hook forcing the byte to 1 gives 52 targets and 270 `RTU` (game-only run), against
    51 and 267.
  - Seen only in compare runs (two processes at once), never in 8 game-only runs.
- trirsz20's rc=1 with everything IDENTICAL was the same byte at **frame 10** (the resize remake).
  Frame 10's log differed on the first pass. compare then redrew frame 10 in fresh processes, the byte
  was zero, so the redraw matched. compare still returns rc=1, which is right.
  - Caught in `re/scratch/taskY3d2b/c2.log`: frame 10, game line 1923 is
    `CreateTexture2D 1280x720 f9` and the port has none. Pixels are IDENTICAL.
- Fix: render_oracle's `pfx_ctor_hook` zeroes +0x131 after the constructor (under
  `RENDER_ORACLE_YEBIS`). This is listed under "Repeatability" in differences.md. rustyAC never makes
  the target.

## Results

Each case is resized at frame 10 (`RENDER_ORACLE_RESIZE`), 14 frames (13 compared), `--post`, the
f2004 lap tape at Spa from 480.

| case | window → resize | frames | call log | pixels |
|---|---|---|---|---|
| s1024 (`--fxaa`) | 1024x770 → 1920x1080 | 13 | IDENTICAL | IDENTICAL |
| s1920 (`--fxaa`) | 1920x1080 → 1250x800 | 13 | IDENTICAL | IDENTICAL |
| s2560 (`--fxaa`) | 2560x1440 → 1024x770 | 13 | IDENTICAL | IDENTICAL |
| s4800 (`--triple`) | 4800x900 → 3072x768 | 13 | IDENTICAL | IDENTICAL |
| trirsz20 ×5 (`--triple`) | 5760x1080 → 3840x720 | 19 | 5/5 IDENTICAL in all 19 frames, rc=0 | 5/5 IDENTICAL |
| 16 `--post` rows (r23_09..24) | as in render.sh | 1 each | 16/16 IDENTICAL (31/31 cases with the overlays) | IDENTICAL |
| `run_all.sh --quick` | — | 340 cases in 27 areas | 340 passed | — |

- s1024 at 1024x770 is the size where the old fit and the code differ (DOF level 160x120, no
  double). Its first run said "4 frames with a different command log" (frames 10-13) on the first
  pass, and the redraw of frame 10 matched. Four reruns with frames 10-13 dumped on the first pass
  (`re/scratch/taskY3d2b/catch.sh`) were IDENTICAL in all 13 frames, so the cause is not known.
  `render_oracle compare` now keeps the first pass's hashes as `<name>.<side>.first.seq` before the
  redraw, so a repeat shows which side changed.

## Not done (Y3d-2c)

- The 38 fixed teardown entries are counted from the c1.log teardown (7 full-size, 25 glare, 2 DOF
  levels, 128x64, 16x16, the AE/AF 1x1s). The UninitializeDevice rule is from code, but the list of
  textures is not walked in code.
- `[POST_PROCESS] QUALITY` other than 0 (base width = QUALITY/5·W) is read but never compared, and
  the other glare/DOF quality tables are refused by yebis_cb.rs (Y5-Y7).
- The s1024 first-pass difference above.

## Open questions

- What made s1024's frames 10-13 differ once, with the AO flag zeroed?
- Does a DOF quality other than 0 change the level-1 rule (`this[0x129]`, the +0x118/+0x11c limits)?

# Task Y3d-2: YEBIS's render targets, clears and teardown (v0.48.5)

## Resume here (Y3d-2)

- **Released as v0.48.5.** Frame 10 is IDENTICAL on both resize cases, and every step-5 proof passed.
  - step 0: the launcher's fixed display-mode list (d089bd0);
  - step 1: frame 0 compared on the `--post` rows (25b0317);
  - steps 2 and 3: the targets, the clear pass and the teardown (cd6506c);
  - step 4: answered by measurement, no code change (see below);
  - step 5: the `--aa` fix (dcbf1ab), then all proofs;
  - the `RENDER_ORACLE_RT_LOG=1` tool in render_oracle (7173e03).
- **Next: Y3d-2b** turns the fitted shortcuts into rules read from the game code: `half_pool`,
  `half_pairs`, `dof_sizes`, and the teardown count (38 + pools + chain). It should also explain the
  run-dependent extra W×H f9 and extra teardown pair (see the open questions). **Heat shimmer stays in
  Y3d-3.**
- Check:
  - `RENDER_ORACLE_RESIZE=10:1024x576 bash re/scratch/taskY3d2/cmp.sh rszfx <name> --track spa --view chase --post --fxaa $T`
  - `RENDER_ORACLE_RESIZE=10:3840x720 bash re/scratch/taskY3d2/cmp.sh trirsz <name> --track spa --view chase --triple --size 5760x1080 --post $T`
  - with `T="--car ks_ferrari_f2004 --tape oracle/audio/ks_ferrari_f2004/trk_lap.audiotape --tape-from 480 --frames 20"`.
  - Then `python re/scratch/taskY3d2/startup.py re/scratch/taskY3d2/runs/<label>` (frame-10 diff).
  - `re/scratch/taskY3d2/end.sh` runs the post rows. `end2.sh` runs both resize compares and then
    `run_all.sh --quick`.
- `render_oracle compare` returns rc=1 on a sequence when any frame differed on its first pass, even
  if the redraw matched. The trirsz20 rc=1 did not come back in two reruns.

## What step 5 fixed (dcbf1ab)

The 7 `--aa` rows had a DIFFERENT static cube map (`start.viewport` 1x1 vs 1280x720). Two causes:
- At startup, `Yebis::new` got no destination. With MSAA nothing is bound there, so the clear pass
  skipped its restores. post.rs `start_yebis` now passes the FXAA target or the screen, and leaves it
  bound without depth, as `initPostProcessing` does.
- The game picks a source view by bind flags (0x1404340b0): an RTV if the texture binds as a render
  target, else a DSV. With MSAA the resolved depth is a colour target (f41), so the game makes an RTV.
  `Yebis::render` now follows the same rule.

## What steps 2 and 3 changed (cd6506c)

- `Yebis::new` makes the targets in the game's order: each texture, then its SRV, then its RTV. The
  R11G11B10 RTVs use a format-0 desc. The game's targets that the port does not draw with (the glare's
  25, TrimCircle, DOF, AE/AF 1x1s, the pool) are made and kept in `_kept`.
- Sizes: `glare_size` (rule above), `glare_pool` (from the rule), `dof_sizes` / `half_pool` /
  `half_pairs` (**shortcuts** fitted to the 1280x720, 3840x720 and 5760x1080 measurements).
- The clear pass in the game's order and groups. Only 24 of the 25 glare targets are cleared, and the
  double-size one is cleared alone as the 20th.
- `teardown(screen)` binds the screen once per released target, plus two more. The count
  38 + pools + chain is a **shortcut** fitted to the RT_LOG runs. post.rs `before_resize` calls it
  before the swap chain is resized (render_oracle and rustyac.exe).
- Both `remade` and `teardown` take YEBIS's destination (the FXAA target, or the screen) from the
  caller, the way the game uses this+0x20, rather than reading the bound target. A kept back-buffer
  view would make `ResizeBuffers` fail.
- YEBIS's depth-stencil desc is zeroed first. A struct literal left stack garbage in its padding, and
  the call log showed it.

## Results (after dcbf1ab)

| case | frames | call log | pixels | textures |
|---|---|---|---|---|
| launcher (step 0) | — | 12/12 IDENTICAL | — | — |
| f0b (`--post`, frame 0 captured) | 0, 1 | IDENTICAL | IDENTICAL | — |
| tri0 / tri10 (no resize) | 1 / 9 | IDENTICAL | IDENTICAL | — |
| rszfx20b (1024x576 resize, FXAA) | 19 (112,954 game calls) | IDENTICAL in all 19 | IDENTICAL | — |
| trirsz20b (5760x1080 → 3840x720, triple) | 19 (229,894 game calls) | IDENTICAL in all 19 | IDENTICAL | — |
| 16 `--post` rows, `TEXTURE_DATA=1` | frame 0 + frame | 16/16 IDENTICAL, cube map included | IDENTICAL | identical |
| `run_all.sh --quick` (0h50m, rc=0) | — | 13/13 areas IDENTICAL; 0 ERROR, 0 skipped, 0 vanished | — | — |

## Step 4: the extra blend / A2C set

There is none in the logs measured today:
- rszfx frame 10 has 39 `OMSetBlendState` calls on each side, 9 of them with a2c=1, and 31
  `CreateBlendState` on each side.
- gpulog writes every `OMSetBlendState` call with no dedupe, so a redundant set would show up as a
  different line.

The Y3d-1 counts (4 vs 5, 3 vs 4) do not reproduce on today's logs.

## The game's target list (from `CPostEffect::Initialize` 0x1403bbb80)

**Creation order** (`InitializeDevice` calls, decomp :2090-2290):
1. render buffer array
2. glare modulator work (+0x120)
3. effect colour alpha dest (+0x118)
4. `CRenderGlare` (+0x108)
5. DOF (+0x110, if this[0x128])
6. objects +0x140..+0x180
7. feedback buffers (+0x188)
8. AE query scene luminance
9. AF query scene depth
10. `CTextureUtil::InitializeDeviceSharedTexturePool` (the pool)

**How each target is made:**
- Each render texture is a bind-0x28 texture, then its SRV, then its RTV, in that order. This is the
  reverse of the port's `target()`.
- f26 (R11G11B10) RTVs are made with a null desc (logged f0).

**rszfx at 1280x720:**
- 3× f9 1280x720 (render buffer: SRV/RTV f10)
- f44 + DSV f45
- 2× f27 1280x720 (f28 views)
- 25× f26 glare targets: 80x45 ×4, 160x90, 80x45 ×20
- 64x64 A8 SRV-only with one upload: `CRenderGlare::CreateDeviceResources` 0x140415520's "TrimCircle",
  `GPUFillTexture_RadiusFade` with radius 0.5 (port: `yebis_tex::trim_circle`, hash 0x5ab5db7501af4869)
- DOF f9 160x90 and 320x180
- 128x64 f27
- 1x1 f27, staging 1x1 f28, 1x1 f27, 1x1 f27, staging 1x1 f28
- pool:
  - f26 640x360 / 320x180 / 160x90
  - f9 160x90 / 320x180 / 640x360
  - f27 64x32 / 32x16 / 16x8 / 8x4 / 4x2

**trirsz at 3840x720:**
- glare 427x16 (854x32 in the 2× slot)
- DOF 160x30 and 320x60
- 128x24 f27
- pool: f26 1920x360 / 960x180 / 480x90, f9 160x30 / 320x60 / 1920x360 / 960x180 / 480x90 / 240x45,
  f27 64x8 / 32x4 / 16x2
- (at 5760x1080 the pool also gains f9 180x34)

**Size rules:**
- **Glare** (:1516-1550):
  - s = sqrt((this+0x2fc / this+0x324) / aspect) = 1
  - a = (long)(w' · DAT_141521650[q·8] · s + 0.99), clamped to ≤ w' and ≤ w; w' is 1280 (flag bit 9 of
    this+0x1dc) in every measured case, and k = 1/16
  - h = (long)(a / ratio + 0.499999)
  - If either a or h is < 16: when a < h, a = 16 and h = (long)(16 / aspect + 0.499999); otherwise
    h = 16 and a = (long)(aspect · a + 0.499999) with the old a (3840x720: 5.333 · 80 → 427).
  - This is why 3840x720 gives 427x16 and not 80x45: Y3c's question is answered.
- **DOF levels** (`CDepthOfField::CreateSystemObjects` 0x1404200a0 :412-459):
  - halve with (float)x · 0.5 + 0.99 per level from the DOF base size;
  - a level that equals the size of the target at +0x200 reuses that target (refcount +1);
  - the last level also gets a half-size partner.
- **Pool lists:**
  - this+0x3e0 (:1400-1450): halvings of e460 with (float)x · 0.5 + 0.99, count
    max(0, ceil(log(max(...)) / log 0.5) − 1).
  - this+0x3f8 (:1452-1515): halving pairs down to 16 wide, made with `AllocateTextures(pool, 2, ...)`.
  - The pool shares a texture when `CRenderTexturePool::CanBeShared` 0x140412020 says so.

## The clear pass (end of `Initialize`, ~:2596-2655)

**Order:**
1. pool `ClearAllGPUResources` 0x140411fb0
2. `CRenderTextureArray::ClearAllGPUResources` 0x1404104e0: the 3 render-buffer targets, each with the
   depth bound and `ClearDepthStencilView 1 1.0 0`, then one restore after all three
3. `CRenderGlare` 0x14041c400, in groups bound with no restore in between
4. DOF 0x14042b930
5. feedback
6. the +0x3e0 list
7. the +0x3f0 pairs
8. `InitializeAutoExposure` 0x1403c83d0
9. `InitializeAutoFocus` 0x1403c8540
10. `SetRenderTarget(this+0x20)` and `SetDepthStencilSurface(this+0x28)`, then
    `IASetPrimitiveTopology 4`

**Each single clear** is `CRenderTexture::ColorFill` 0x1403d2ff0:
- bind the target and its viewport, then `ClearRenderTargetView 0`;
- then bind the screen and its full viewport again.

The owners of pool-shared targets clear them again after the glare. The exact sequence for both sizes
is in `clears.py`'s output for `runs/rszfx` and `runs/rtlogtri`.

## The teardown (step 3)

- At the resize, `CPostEffect::Uninitialize` 0x1403c08a0 calls `CRenderTexture::UninitializeDevice`
  0x1403d29f0 once per target. Each call binds the screen with no depth plus its full viewport.
- The game's frame 10:
  - 1 + N + 1 pairs of `OMSetRenderTargets 1 [screen] -` plus the viewport, then `OMSetRenderTargets 0 [] -`;
  - rszfx: 52 pairs on the 1280x720 f28 screen;
  - trirsz: 54 pairs on the 5760x1080 back buffer.
- The port emits OM 0 at line 35 instead.
- Port plan:
  - save the bound screen RTV and size where `Yebis::new` rebinds it (~yebis.rs:465);
  - add `teardown()`, called from `post.rs` `on_resize` before `forward.on_resize` (the old screen
    must still be bound).
- `RENDER_ORACLE_RT_LOG=1` logs every `SetRenderTarget` ("RTS") and `UninitializeDevice` ("RTU") with
  callers. The runs are in `runs/rtlog` and `runs/rtlogtri`.

## Open questions

- The 64x64 A8 upload's data source: answered (TrimCircle, above).
- Some game runs make an extra W×H f9 after the DOF levels (runs/rs3, runs/trirsz, Y3d-1's rszfx) and
  others do not (runs/rszfx, runs/rtlogtri), with the same arguments and no `TEXTURE_DATA`. The port
  never makes it. What decides it is not known (the per-thread `rand()` is a guess).
- The pool's f9 list and the pairs' clear order are measured shortcuts (`half_pool`, `half_pairs`);
  `CRenderTexturePool` sharing was not derived.

# Task Y3d-1: YEBIS's device states, generated textures and buffers in the game's order

## Resume here

- **Released as v0.48.4.** `Yebis::new` now creates, in the game's order:
  - `IGPUDevice::InitializeDevice`'s states: the DrawRect raster, depth and blend, 30 blend/raster pairs
    and 16 samplers;
  - the screen bind and its viewport;
  - `CTextureUtil::InitializeDevice`'s textures, filled with the game's maths;
  - the 1,541 buffers.

  Shaders, the B4096 cbuffer and the input layouts are created on first draw, as the game does.
- **Next: Task Y3d-2** (render-target pool, the 49 clears, the teardown; see "What next needs").
- Check: run `RENDER_ORACLE_TEXTURE_DATA=1 bash re/scratch/taskY3d1/cmp.sh rszfx rszfx <args>`, with the
  rszfx/trirsz arguments from `re/scratch/taskY3b/regress.sh`. Then run
  `python re/scratch/taskY3d1/startup.py re/scratch/taskY3d1/runs/rszfx`. It compares game and port
  from frame 10's first `CreateRasterizerState`, and counts differing screen-bind lines without
  stopping at them.
- Disk: C: had 21 GB free at the start and 21 GB before the proof runs.

## What it found

1. **gpulog `RENDER_ORACLE_TEXTURE_DATA=1`** (crates/rustyac-render/src/gpulog.rs). After each texture
   `UpdateSubresource`, the texture is read back and renamed by its new contents, so the upload line
   names the hash of what was uploaded. It is off by default, so the default proofs are unchanged.
2. **Device states** (`IGPUDevice::InitializeDevice` 0x1403d4610):
   - The DrawRect depth state is off, write mask ZERO, compare ALWAYS.
   - The blend state has only RT0 filled; RT1–7 are zero bytes.
   - The 30 pairs are blend first, then raster.
   - The 16 samplers have MaxAniso 1 and comparison 0. The port's five samplers are game samplers 1,
     4, 5, 13 and 0.
   - `InitializeDeviceBuffers` takes the bound target (OMGetRenderTargets), rebinds it and sets a
     viewport of its size.
3. **Generated textures** (crates/rustyac-render/src/yebis_tex.rs; `CTextureUtil::InitializeDevice`
   0x1403e6890). Each one is made as SRV first (MipLevels −1), then one upload per mip:
   - **film grain:** `ppfx_GaussianNoise`, read from acs.exe at file offset 0x52fa20;
   - **spectrum:** `GPUMath_Wavelength_to_RGB` over the CIE tables at 0x151f600 and 0x151f360;
   - **mipmaps:** `GPUTexUtil_Build2DMipmaps` with its 4x4 dither bias;
   - **Gaussian:** the 64x64 A8 radius fade;
   - the three 1x1 textures, the 16x16 depth and the analyzer.

   The maths is MSVCR120's `expf`, `logf` and `powf` from rustyac-math. No new CRT functions were
   needed. The normalising maximum (`FUN_1403add60`, a static initializer) is recomputed from the table.
4. **Buffers:** 768× B192 (the ring), B8, 768× B32, 3× B45056 and B3072, in that order. The two
   buffers with initial data hash the same as the game's.
5. **Lazy creation:** the game creates each VS, then the B4096 cbuffer on the first draw, then the PS.
   The input layout comes after the cbuffers are bound. Identical bytes share one object, so frame 10
   has 5 VS, 7 PS and 3 layouts, not 8 each.
6. **Frame 10 of the resize runs.** The first difference is now the first render-target pool entry
   (rszfx game line 1823, trirsz 1828), as planned. The screen-bind lines still differ: the game
   rebinds its 1280x720 back buffer without depth, but the port has KGL's target with its depth bound.
   The game binds the screen during its teardown (`CRenderTexture::UninitializeDevice`), which the
   port does not have yet (Y3d-2). The game has one more texture upload than the port: a 64x64 A8 at
   line 1917, inside the pool (Y3d-2).

| Case | Frames compared | Call log | Pixels | Textures |
|---|---|---|---|---|
| 16 `--post` rows r23_09..r23_24 (render.sh) | each row's frames | IDENTICAL (16/16) | IDENTICAL (16/16) | not hashed (default) |
| rszfx (resize at frame 10, start-up from line 152) | 20 | start-up IDENTICAL to line 1822 (2 screen-bind lines); whole log DIFFERENT (Y3d-2) | IDENTICAL | 28 uploads IDENTICAL |
| trirsz (same, triple 5760x1080) | 20 | start-up IDENTICAL to line 1827 (1 screen-bind line); whole log DIFFERENT (Y3d-2) | IDENTICAL | 28 uploads IDENTICAL |
| `run_all.sh --quick` (13 areas, 1h00m) | each case's | 169 of 170 IDENTICAL; launcher quickrace DIFFERENT once (live display-mode list, 1465 vs 489 bytes), IDENTICAL 1984/1984 when rerun alone | IDENTICAL | - |

Frame 10 calls:

| Case | Before | After |
|---|---|---|
| rszfx | start-up identical 152..152; 7,376 lines differ, first at 35 | start-up identical 152..1822; 6,622 lines differ, first at 35 (the teardown) |
| trirsz | start-up identical 155..155; 13,714 lines differ, first at 34 | start-up identical 157..1827; 12,945 lines differ, first at 34 |

## What next needs (Task Y3d-2)

- The render-target pool, the array, the glare, DOF and luminance allocations, the resource manager's
  creation order and the 49 clears (Y3c item 1). This includes the 64x64 A8 at game line 1917.
- The teardown (`CPostEffect::Uninitialize` → `CRenderTexture::UninitializeDevice`). It also fixes
  the screen-bind lines: the game binds its back buffer with no depth.
- The one extra blend and A2C set in the port's YEBIS draws (4 vs 5 and 3 vs 4 against the game), not
  investigated.
- Heat shimmer (Y3d-3) and VR with YEBIS, unchanged.

## Not done

- Y3d-2 and Y3d-3 above.
- Frame 0's start-up is still not compared: capture starts at frame 1. Only the frame-10 remake
  checks it.

## Open questions

- Why the port sets the blend and A2C state once more than the game in its YEBIS draws (pixels agree).

# Task Y3c: YEBIS's setup and teardown mapped in game code; step 1 stopped before it (rule 7)

## Resume here

- **Not released (no code change).** Step 1 is about 60 KB of game code, not the expected ~30 KB, so
  it was stopped before it under rule 7. Steps 2 and 3 depend on it. The whole of it moves to **Task
  Y3d**, split as below. Heat shimmer and VR with YEBIS move with it.
- Check (unchanged): `bash re/scratch/taskY3b/regress.sh`. Frame-10 logs of the resize run are in
  `re/scratch/taskY3/runs/rszfx/*.f10.gpulog`. `re/scratch/taskY3c/init_skim.c` is
  `CPostEffect::Initialize` with prints and declarations removed (scratch only, not committed).
- Disk: C: had 21 GB free at the start and 21 GB at the end (no build or run was made).

## What it found

1. **Who creates what on a remake** (the order in game frame 10 of rszfx, log lines 141–2200):
   - The KGL resize (lines 141–151, already ported).
   - `IGPUDevice::InitializeDevice` 0x1403d4610 (lines 152–232), about 3.5 KB:
     - `…_DrawRectDefaultStates` 0x1403d48c0: raster, depth, blend.
     - `…_RenderStateMode` 0x1403d4b00: for p in 0..64, a blend then a raster, only when
       blend = p&7 ≤ 4 and write = p>>3&3 ≤ 2 (30 pairs). Blend modes: 0 (off, 2/2/1), 1 (5/6/1), 2
       (2/2/1), 3 (2/4/1, alpha 2/6/1), 4 (2/0xe/1). Write masks: 0xf, 7, 8. Scissor = p>>5&1.
     - `…_SamplerStateMode` 0x1403d4dd0: 16 samplers. kind p&3 is filter 0x14/0/0x14/0x15 with MaxLOD
       0/0/FLT_MAX/FLT_MAX. Address p>>2&3 is border/clamp/mirror/wrap (W is always clamp). The
       port's SB, SL, SP, SW and SLB are game samplers 1, 4, 5, 13 and 0.
     - `InitializeDeviceBuffers` 0x1403d4330: OMGetRenderTargets, then binds the screen and its
       viewport (lines 231–232).
     - The D3D11 runtime normalizes bound state descriptions, so creating the states with the game's
       exact bytes leaves every per-frame log line as it is.
   - `CTextureUtil::InitializeDevice` 0x1403e6890 (4,944 B, reached through a vtable) creates:
     - lines 233–281: a 16x16 f27 target; the 4x4 dither; film grain 256x256 with 9 mips; spectrum
       256x1 with 9 mips; Gaussian 64x64 A8 with 5 mips; three 1x1 textures; a 16x16 depth; a 16x16
       analyzer.
     - lines 282–1822: 768× B192 (the port's ring), B8 with data, 768× B32, 3× B45056, and B3072 with
       data.
   - Render targets (lines 1823–1970, about 40) are allocated through `CRenderTexturePool`
     (`AllocateTextures` 0x140411900, `CanBeShared` / `PostTexture`, which decide how many distinct
     80x45 targets exist), `CRenderTextureArray` (0x14040ede0 / 0x14040f710), the glare and DOF
     `CreateSystemObjects` (0x140413c00 6,432 B, 0x1404200a0 9,904 B) and the luminance managers
     (0x140432ea0 / 0x140431b70). They are then created on the device in `CGPUResourceManage`
     registration order (`ResetDevice` 0x1403dd640, `InitializeDevice` 0x1403dd8b0). The whole run is
     driven by `CPostEffect::Initialize` 0x1403bbb80 (19,744 B).
   - **The 49 clears** come from `CRenderTexture::CreateDeviceResources` 0x1403d1280 (2,320 B), which
     clears when `ms_bClearWhenInitializeDevice` is set (`CPostEffect::Initialize` sets it). Each clear
     binds the target and viewport, clears to 0 and rebinds the screen. Some runs of 80x45 targets rebind the screen only
     once; why is not traced yet.
2. **The teardown's source.** `CPostEffect::Initialize` first calls `CPostEffect::Uninitialize`
   0x1403c08a0 (4,048 B). That releases each render texture through `CRenderTexture::UninitializeDevice`
   0x1403d29f0 (1,328 B), which:
   - takes the default surface (device vtable +0x58);
   - binds it as target 0 (+0x30), which logs the OMSet screen and viewport pair;
   - sets targets 1–3 and the depth to null (+0x30 / +0x38).

   So a remake really is teardown plus setup inside one `Initialize`. `IGPUDevice::ReleaseDeviceBuffers`
   0x1403d55f0 and `CTextureUtil::UninitializeDevice` 0x1403e7c10 release the rest.
3. **Size.**

   | Part | Code |
   |---|---|
   | device states | ~3.5 KB |
   | CTextureUtil | ~5.8 KB |
   | CPostEffect::Initialize | 19.7 KB |
   | glare | ~7.3 KB |
   | DOF | ~11.3 KB |
   | luminance | ~2.1 KB |
   | pool, array, render texture and resource manager | ~10 KB |
   | teardown | ~6.5 KB |
   | **total** | **about 66 KB, against the ~30 KB expected** |

4. **A gap in the proof.** The oracle names a texture by a hash read back when its first view is
   created, which is before `UpdateSubresource`, so the hash is of an empty texture. The generated
   textures' contents (film grain, spectrum, Gaussian, the 64x64 A8 at line 1915) therefore never
   reach the call log. Only the B8 and B3072 buffers are checked by content. Proving the CRT maths
   needs a gpulog option, behind an env var, that hashes texture `UpdateSubresource` data.
5. **`rand()` for heat shimmer.** acs.exe has 39 `rand` call sites and 4 `srand` sites. Some are
   `ksRand` 0x140033770 / 0x1400337c0, which `CarBrakeLights::loadLights` and others use. The
   `ksRandomize` 0x14004b290 site sits in `ACClient::handshakeTCP`, and `CameraFacing::CameraFacing`
   0x14005e640 and `PhysicsDriveThread::run` / `step` call `srand` themselves. MSVCR120 keeps the
   `rand` state per thread (`_holdrand` in the thread data). So the physics thread's draws never mix
   with the render thread's, and the shimmer only has to follow the render thread's sequence.

| Case | Frames compared | Call log | Pixels |
|---|---|---|---|
| all (no code change; nothing run) | - | - | - |

## What next needs (Task Y3d, split)

- **Y3d-1, about 1 session.** Port the device states, `CTextureUtil::InitializeDevice` (textures with
  their CRT maths, plus the 1541 buffers) and the screen bind into `Yebis::new`, in the game's order.
  Map the port's five samplers to game indices 1, 4, 5, 13 and 0. Add the gpulog texture-data hash.
  Frame 10's first difference should then move from line 141 to the first render target at line 1823.
- **Y3d-2, about 2 sessions.** Port the render-texture side: the pool's sharing rules, the array, the
  glare, DOF and luminance allocations, the resource-manager creation order and the 49 clears. The
  port's effect, tonemap, luminance, analysis and depth targets become entries of that set. Then port
  the teardown (`CPostEffect::Uninitialize` → `CRenderTexture::UninitializeDevice`), so a remake is
  `Uninitialize` plus `Initialize`. Check: rszfx and trirsz frame 10 IDENTICAL, then the 16 `--post`
  rows and `--quick`.
- **Y3d-3, heat shimmer, about 1–1.5 sessions, after Y3d-1** (it needs the CTextureUtil textures):
  - The updater 0x1400abc20 and `KGLYebisParticle::initialize` 0x140024600, which draws seven
    `rand()` per particle.
  - The four PPFX functions and their programs.
  - A root with `HEAT_SHIMMER=1` and a slow tape segment.
  - For `rand()`, one render-thread `MsvcRand` (crates/rustyac-physics/src/session.rs), starting at 1
    like MSVCR120's per-thread default. Pass it by `&mut` to every render-thread caller in the game's
    order (`ParticleGenerator::generateParticle`, `SkyBox::updateCloudsGeneration`,
    `ACCameraManager::stepCameraRandom`, `CameraModeStart::updateCar`, `ksRand` users, the shimmer
    particles), applying `CameraFacing`'s and `ksRandomize`'s `srand` where the game calls them.
    Physics-thread callers (AI, drag, pit, sessions) keep their own state.
  - Before porting, list the render-thread callers that are live in the proof's frames, for example by
    counting calls through the `rand` import slot in an oracle run. A frame with no other live
    caller makes a first proof possible without the rest.
- VR with YEBIS (`YebisPP`): about one session, unchanged.

## Not done

- Steps 1–3: the setup port, the teardown port, the runs and the v0.48.4 release. All of them were
  stopped under rule 7 with the estimate above.
- Heat shimmer and VR with YEBIS (Task Y3d).

## Open questions

- Whether `CRenderTexturePool::CanBeShared` gives the same sharing at every window size, or whether
  the number of 80x45 targets changes with the size (to be checked in trirsz at 3840x720).
- Which render-thread `rand()` callers are live in a shimmer proof's frames: particles and sky clouds
  likely are.

# Task Y3b: the two Y3 shortcuts from game code; the resize remake and heat shimmer stopped (rule 7)

## Resume here

- **Released as v0.48.3.** The two Y3 shortcuts are now rules read from game code (step 2). Step 1 (the
  resize frame's call log) and step 3 (heat shimmer) were stopped under rule 7 and moved to **Task Y3c**,
  together with VR with YEBIS.
- Check: `bash re/scratch/taskY3b/regress.sh` runs rszfx, trirsz, tri and aeoff one after another
  (through `re/scratch/taskY3/cmp.sh`) and writes `re/scratch/taskY3b/regress.txt`.
- Disk: C: had 23 GB free at the start and 19 GB before the runs (under the 22 GB mark). Free RAM was
  11.1 GB before the runs; the quick run still used its full lanes (it checks at its own start).

## What it found

1. **The linear filter rule** is `CTextureUtil::Resample` 0x1403f6710. The second texture's sampler mode
   is set through `IGPUBaseTexture::SetSamplerStateMode_Filter` 0x1404365b0 (vtable +0x68, `(mode & ~3) |
   filter`). It is point when that texture's size equals the destination's (+0x5c / +0x60), and
   otherwise linear (min/mag linear, mip point). The address bits (border) are kept. Draw 0 binds
   [depth, scene], so the scene is that second texture, and the port's size test was already the
   game's. Only the comment changed.
2. **The vignette on a triple-screen resize.** The `CameraTripleScreen` ctor 0x1402248e0 registers
   lambda 0x140224dc0 on `evOnPostYebisParametersUpdate` (CameraForwardYebis +0x788). It is fired in
   `CameraForwardYebis::render` 0x14002a450 at 0x14002a57c, right after `updateYebisParameters`. So
   the vignette is zero at a remake only once a frame has been rendered. `post.rs` now keeps
   `parameters_updated` and tests it in `on_resize`, which replaces the "no caller does that" marker.
3. **The resize remake (step 1) is bigger than its Y3 estimate.** Frame 10's pixels were already
   IDENTICAL in rszfx and trirsz. What differs is only PPFX's full device setup:
   - IGPUDevice states: `DrawRectDefaultStates`, the 0x40-mode `RenderStateMode` loop (5 blend modes ×
     3 write masks × scissor), and 16 `SamplerStateMode`.
   - `CTextureUtil::InitializeDevice` 0x1403e6890 (4,944 B): film grain 256x256 with 9 mips, spectrum
     256x1 with 9 mips, Gaussian fade 64x64 A8 with 5 mips, zero / grey / one, a 16x16 depth, and a
     16x16 analyzer, all with exact CRT math.
   - `IGPUDevice::InitializeDeviceBuffers`: 768× B192, 768× B32, 3× B45056 and B3072.
   - `CPostEffect::Initialize` 0x1403bbb80 (19,744 B): about 40 targets (glare, DOF and luminance
     managers) and 49 clears.
   - 52 OMSet + viewport pairs during teardown, whose source was not found.
4. **Heat shimmer (step 3) is bigger than its Y3 estimate.**
   - `PostProcessEffectsUpdater::update` 0x1400abc20 is small: it is off above 25 km/h for the focused
     car, and it stops at the first car that is not connected. For each car under 30 it places a volume
     at the body matrix times (0, 0, -2, 1), plus 0.5 in y, with width 1.5, depth 1.5 and height 0.
   - The particles in those volumes use CRT `rand()` (`KGLYebisParticle::initialize` 0x140024600:
     seven calls per particle), so the port's rand sequence would have to match every other `rand()`
     user in the frame.
   - The effect itself is about 8.8 KB of PPFX: `HeatShimmerParticles` 0x1403fdc70 (1,392),
     `SetupHeatShimmerParticles` 0x1403fd860 (1,040), `DrawIndicesGPU_HeatShimmerParticles` 0x1403fc950
     (3,856), and `UpdateParameters_EffectiveHeatShimmerParticleEnable` 0x1403ba2e0 (464).
   - Its default textures (CTextureUtil +0x21668 / +0x21678) come from step 1's unported init.
   - A proof also needs a new run: the oracle root has `video.ini HEAT_SHIMMER=0`, and the trk_lap tape
     from frame 480 is above 25 km/h.

| Case | Frames compared | Call log | Pixels |
|---|---|---|---|
| rszfx, resize to 1024x576 at frame 10, FXAA | 19 | 18 IDENTICAL; frame 10 DIFFERENT (7981 vs 6684 calls, same 864 draws; step 1) | IDENTICAL in all |
| trirsz, triple, resize to 3840x720 at frame 10 | 19 | 18 IDENTICAL; frame 10 DIFFERENT (14315 vs 12992 calls, same 1590 draws; step 1) | IDENTICAL in all |
| tri, triple 5760x1080 `--post` | 19 | IDENTICAL in all | IDENTICAL in all |
| aeoff, spa chase, `ae_off` filter | 38 | IDENTICAL in all | IDENTICAL in all |
| heat shimmer (step 3) | not run | - | - |

`tools/proofs/run_all.sh --quick`: all 170 cases IDENTICAL in 13 areas, no error, skip or vanished case
(1h04m). Clippy `-D warnings --all-targets` is clean natively, with `--features wgpu`, and on both wasm
targets; `cargo test -p rustyac-render --lib yebis` passes.

## What next needs (Task Y3c)

- Step 1: port PPFX's device init in its order (the list above), make the shaders and the B4096 cbuffer
  lazy as now, and find the teardown's 52 OMSet / viewport pairs (likely `CPostEffect::Uninitialize` /
  `OnLostDevice`, or the IGPUDevice release). About 1.5 to 2 sessions.
- Step 3, after step 1 (it needs the CTextureUtil textures):
  - the updater and `KGLYebisParticle*` (initialize, set update, manager setup) on the port's CRT `rand()`
  - the four PPFX functions and the `HeatShimmerParticle_Sn` / `MadT1_Sn` programs
  - a root with `HEAT_SHIMMER=1` and a slow tape segment (a standing start)

  About 1 to 1.5 sessions.
- VR with YEBIS (`YebisPP`), unchanged from Y3: about one session or more.

## Not done

- Step 1 (frame 10's call log on resize) and step 3 (heat shimmer), both stopped under rule 7 with the
  estimates above.
- VR with YEBIS (Task Y3c, as planned).

## Open questions

- Where the 52 OMSet + viewport pairs at YEBIS teardown come from.
- Whether the port's `rand()` order can match the game's for the shimmer particles, or whether the
  proof needs a run with no other `rand()` users in the frame.

# Task Y3: YEBIS on resize, auto exposure off, triple screens; the remake, VR and heat shimmer stopped (rule 7)

## Resume here

- **Done, released as v0.48.2.** YEBIS now sizes its luminance target and analysis chain from the window
  like the game (`luminance_sizes` / `box_taps` in `yebis.rs` / `yebis_cb.rs`), picks the analysis
  program by tap count (`MadT1_S1` / `_S2` / `_S5`), uses the no-vignette tonemap programs when the
  vignette is zero (triple screens), keeps the FXAA target at its old size on resize, and draws nothing
  for the luminance with auto exposure off.
- Next is **Task Y3b**: the three steps stopped under rule 7 (below), each about one session.
- Check one run: `bash re/scratch/taskY3/cmp.sh <label> <out> <render_oracle compare args>` writes
  `re/scratch/taskY3/<out>.txt`; `re/scratch/taskY3/regress.sh` reruns the cases below one after another.
  The `ae_off` filter is `default` with `[AUTO_EXPOSURE] ENABLED=0`, in the scratch root
  `re/scratch/taskY2c/root/system/cfg/ppfilters/`.
- Test: `cargo test --locked -p rustyac-render --lib yebis`.
- Disk: C: had 24 GB free; `tools/proofs/archive_kept.sh` (dry run) found 0 old kept folders, so there was nothing to move.

## What it found

1. **Luminance size** (`CPostEffect::Initialize` 0x1403bbb80): power-of-two floor of `W>>3` by `H>>3`;
   when wider than 128 it is 128 by `(int)(128/aspect + 0.49999)`. 1280x720 gives 128x64, 5760x1080
   gives 128x24 (the port had 128x64 hard-coded; the triple log differed there).
2. **Analysis chain** (`CTextureLuminance::CreateSystemObjects` 0x140431b70) and its taps: per axis
   `n = ceil(s/2/d)`, one fewer when `n > 1`, `d ≥ 8` and the edge rule holds, at most 5; 1, 2 or 5 taps
   choose `MadT1_S1`, `MadT1_S2` or `MadT1_S5`. The analysis and no-vignette programs are made lazily on
   their first draw, as the game does.
3. **Triple screens zero the vignette** (CameraTripleScreen handler 0x140224dc0) and
   `DrawRectGPU_TonemapHDR` 0x1403f1b10 then uses `TonemapHDR_PreMatrix_Dither_Gamma` and
   `TonemapHDR_Dither_Exposure_LINEARSAT` with an all-zero rectangle.
4. **Resize keeps the FXAA target** at its first size, and YEBIS is remade at that size, so the effects
   run at the old size and the result is scaled into the new window. Not seen as a visible fault in the
   runs; the port does the same.
5. **The scaled scene uses the linear sampler** when the scene and YEBIS sizes differ (rule read from the
   game's logs, `shortcut:` in `yebis.rs`).
6. **Auto exposure off** draws no luminance step; proven
   over 38 frames, its `shortcut:` is gone.
7. **gpulog**: a texture created at a released buffer's address was taken for that buffer; fixed.

| Case | Frames compared | Call log | Pixels |
|---|---|---|---|
| aeoff, spa chase, `ae_off` filter | 38 | IDENTICAL in all | IDENTICAL in all |
| tri, triple 5760x1080 `--post` | 19 | IDENTICAL in all | IDENTICAL in all |
| rszfx, resize to 1024x576 at frame 10, FXAA | 20 | 19 IDENTICAL; frame 10 DIFFERENT (7981 vs 6684 calls, same 864 draws) | IDENTICAL in all |
| trirsz, triple, resize to 3840x720 at frame 10 | 19 | 18 IDENTICAL; frame 10 DIFFERENT (14315 vs 12992 calls, same 1590 draws) | IDENTICAL in all |

`tools/proofs/run_all.sh --quick`: all 170 cases IDENTICAL in 13 areas, no error, skip or vanished case (1h00m). Clippy `-D warnings --all-targets` is clean natively and on both wasm targets; render_oracle's own `--all-targets` clippy still has its 25 old errors (none from this task).

## Stopped under rule 7 (Task Y3b)

- **The resize frame's remake (step 1, call log of frame 10).** The game's YEBIS init rebuilds its whole
  device setup: CreateBuffer 1542 vs 770, render-target views 51 vs 13, Texture2D 64 vs 20,
  ClearRenderTargetView 49 vs 0, blend / rasterizer states 31 vs 0, all inside PPFX's own setup. Pixels
  and the draw sequence already match. About half a session to one session.
- **`PostProcessEffectsUpdater::update`** 0x1400abc20 (step 2) drives the heat shimmer: active when the
  focused car is under 25 km/h, a projected particle per connected car (up to 30), from `[HEAT_SHIMMER]`
  (`ENABLED=1` in default.ini and most filters). Modelling it means porting a new YEBIS effect (the
  shimmer draws). About one session.
- **VR with YEBIS (step 3).** `YebisPP` 0x14002ba30 is its own class, made by `StereoCameraForward`
  0x140211370 and `StereoCameraVive` 0x140220b80: one instance per camera, shared by both eyes.
  `StereoCameraVive::renderYebis` 0x140223f60 runs per eye: render into `rtYebisTarget`,
  `updatePPCameraValues`, zero `+0x5a4` / `+0x48`, `updatePP(eye==0)`, `applyPostProcessing` (optional
  MSAA and depth resolve) into the eye's target or through `rtFXAA_Temp` and an FXAA quad. Its
  `readPPSet` 0x14002cd60 is 14 KB on its own. About one session or more, plus oracle runs with the
  OpenVR / Oculus stand-ins. Triple screens are done (one YEBIS over the whole window, as the game).

## What Y4 (the page) needs

- The page still uses the plain-copy stand-in. YEBIS's programs can't ship, so the page needs them from
  the player's own install (as the desktop reads `acs.exe`) and translated for wgpu, then the same rules
  as here: `luminance_sizes`, `box_taps`, the lazy programs, the FXAA target kept on resize, and a
  luminance read-back that fits WebGPU's asynchronous buffer mapping.

## Not done

- The three rule-7 steps above.
- Glare, depth of field, rays of god and heat shimmer (filters with them are refused).

# Task Y2c: YEBIS over many frames, eye adaptation and a window resize

## Resume here

- **Done, released as v0.48.1.** YEBIS now runs every frame like the game: the luminance analysis and
  read-back, eye adaptation (`AutoExposure` in `yebis_cb.rs`), and a remake on window resize
  (`CameraForwardYebis::on_resize`). The proofs' `--post` rows run the real YEBIS on both sides
  (`tools/proofs/render.sh` sets `RENDER_ORACLE_YEBIS=1`); r23_09 is in `--quick`, every `--post` row is in `--full`.
- Superseded: Task Y3 above did Y2d's leftovers and triple screens.
- Check one run: `bash re/scratch/taskY2c/cmp.sh <label> <out> <render_oracle compare args>` writes
  `re/scratch/taskY2c/<out>.txt`. New oracle inputs: `--ppfilter <name>` (video.ini `FILTER=`) and
  `RENDER_ORACLE_RESIZE=<frame>:<W>x<H>` (resizes the window before that frame's capture; dump only frames
  after it, the PNGs use the last size).
- Test: `cargo test --locked -p rustyac-render --lib yebis`.

## What it found

1. **The ppfilter is loaded by loadPPSet** 0x140029960 from video.ini's `FILTER`. The oracle's game side
   now calls it with `--ppfilter`, so a second filter (`default_dark`, filmic) is proven.
2. **The vertex ring starts on buffer 767**, so the first map of a run is the wrap's discard.
3. **Frame 0 differs on purpose**: the game makes YEBIS's shaders and layouts lazily on first use.
   Sequences capture from frame 1, as before.
4. **Eye adaptation** is `CTextureLuminanceManage::UpdateFrame` 0x140433230: a 17-slot history. The
   luminance target is read back (staging copy of the 1x1), weighted 0.2126 / 0.7152 / 0.0722 times
   0.003921569, decoded with the filmic `-logf(1-x)`, smoothed in log space with
   `rate = 1 - powf(0.050000012, dt*6/0.1)` and lerped between slots. The analysis is two draws,
   `MadT1_S1` (2:1 resample) and `MadT1_S2` (4x2 to 1x1). `logf` now comes from MSVCR120 like the other
   CRT functions. Exposure on the La Source run (default_dark), game = port:

   | Frame | 20 | 60 | 100 | 140 | 180 | 220 | 239 |
   |---|---|---|---|---|---|---|---|
   | Exposure | 0.4846618 | 0.3045707 | 0.2810263 | 0.2782328 | 0.2686784 | 0.2682259 | 0.2686277 |

5. **Resize**: `GraphicsManager::onResize` 0x140204330 calls `kglResizeBuffers` 0x1400193d0
   (`ResizeBuffers(0,0,…)` takes the window's client size, so the window is sized first), then
   `CameraForwardYebis::onResize` 0x140029da0 remakes YEBIS. The noise is process-wide and survives. The
   remade 1x1 textures and the 4x4 dither are created empty, then get their view, then `UpdateSubresource`
   (gpulog keys a texture made inside a capture by that order).
6. **With FXAA on**, start leaves YEBIS's destination (the FXAA target) bound, not the screen. This
   fixed the 5 FXAA rows' first cube-map line.

| Case | Frames compared | Call log | Pixels |
|---|---|---|---|
| 16 `--post` rows r23_09..r23_24 | 1 to 59 each | IDENTICAL | IDENTICAL |
| f16, spa chase | 15 | IDENTICAL in all | IDENTICAL in all |
| lsrc, La Source, default_dark, light changes | 239 | IDENTICAL in all | IDENTICAL in all |
| rsz, resize to 1024x576 at frame 10 | 19 | 18 IDENTICAL; frame 10 DIFFERENT (7961 vs 6672 calls, same 863 draws) | IDENTICAL in all |

`tools/proofs/run_all.sh --quick`: all 170 cases IDENTICAL in 13 areas, no difference, no error (r23_24 left the quick set for r23_09). Clippy `-D warnings --all-targets` is clean natively and on both wasm targets.

## What Y2d / Y3 need

- **Y2d:** the resize frame's resource creation (the game remakes far more inside its YEBIS init: 1542 vs
  770 `CreateBuffer`, 50 vs 13 RTVs); the auto-exposure-off path (no luminance step is drawn, unproven:
  `shortcut:` in `yebis.rs`); `PostProcessEffectsUpdater`'s per-frame update; the game keeps its FXAA
  target at the old size on resize while the port remakes it; render_oracle's 27 old clippy errors
  (`--all-targets`, from before Y2c).
- **Y3 (VR and triple with YEBIS):** YEBIS per eye / per screen: the targets and the luminance history
  are per `Yebis` today, sized to one view. VR's two eyes and triple's three viewports need either one
  `Yebis` per view or the game's own sharing, found in its VR/triple camera classes, plus oracle runs
  with `--vr` / `--triple` and `--post`.

## Not done

- Glare, depth of field, rays of god and heat shimmer (filters with them are refused).
- YEBIS in the graphics preset (post-processing stays off, decided) and in the browser (stand-in until Y4).

# Task Y2b: YEBIS's 8 draws on the desktop, proven against the game

## Resume here

- **Done, released as v0.48.0.** `crates/rustyac-render/src/yebis.rs` draws YEBIS's frame (both rounds and
  the composite) on Direct3D 11. `CameraForwardYebis::start_yebis` (`post.rs`) reads it from the player's
  `acs.exe` and the ppfilter named in video.ini. `rustyac.exe --post 1` and `render_oracle --post` with
  `RENDER_ORACLE_YEBIS=1` use it. Without `start_yebis` (wgpu, the browser, the proofs' default rows) the
  plain-copy stand-in stays.
- Next is **Task Y2c** (leftovers below), then Y3 (eye adaptation) and the rest of the Y1 plan.
- Check one scene: `bash re/scratch/taskY2b/cmp.sh r23_09 out --track spa --view chase --post` (also
  r23_10 with `--aa 4`, r23_16 with `--track monza --ldr-levels`). It writes `re/scratch/taskY2b/out.txt`.
- Test: `cargo test --locked -p rustyac-render --lib yebis`. The uploads test reads the game logs in
  `re/scratch/taskY2b/runs/` (git-ignored) and skips without them.

## What it found

1. **All three scenes are IDENTICAL**: call log with no ignored bit, and pixels.
2. **The planted change is caught.** A tone-map vignette 1% larger makes r23_09 DIFFERENT.
3. **YEBIS reads two surfaces in turn.** Its render-texture array (`CPostEffect`+0x100) holds 2 effect
   surfaces. A draw never reads the surface it writes, because Direct3D 11 unbinds such a read and it
   returns 0. Each round tone-maps into its own 8-bit target. The composite reads round 1's colour (slots
   0-2) and round 2's alpha (slots 3-5).
4. **The vertex ring** is `CTextureUtil`'s 768 dynamic buffers of 192 bytes. A draw goes to the next
   buffer when it would reach byte 192. Only the wrap past buffer 767 is a real discard; every other map is
   `NO_OVERWRITE`. This is why each wrap in the log shows a buffer of zeros plus the new vertices.
5. **The dither** is `CTextureUtil::InitializeDevice` 0x1403e6890's 4x4 Bayer texture. **The noise** is
   four linear congruential states (seeds 1, 0x3391, 0x61e9b, 0xbf1), stepped once a frame from the
   second frame on; both tone maps of a frame use the same values.
6. **The screen after start.** `initPostProcessing` leaves the back buffer bound without depth, outside
   kgl's cache. The port does the same, and the static cube map's first `OMSetRenderTargets` then matches.
7. **Draw 8's alpha sign.** The game's composite weight is -0 while `CRenderGlare`+0x108 is set. The
   oracle zeroes it (`differences.md`, Repeatability), so the Y2a test needs no mask either.

| Scene | Call log | Pixels |
|---|---|---|
| r23_09 spa chase | IDENTICAL | IDENTICAL |
| r23_10 spa chase MSAA 4 | IDENTICAL | IDENTICAL |
| r23_16 monza chase, LDR levels | IDENTICAL | IDENTICAL |
| r23_09 with the planted change | DIFFERENT (2 lines: the draw 4 vertex upload) | DIFFERENT (6300 pixels, by 1) |

`tools/proofs/run_all.sh --quick` (post-processing rows without YEBIS): all 170 cases IDENTICAL in 13 areas, no difference, no error (browser and ai_page ran again alone: the first run's page build got a second `--sysroot` from the check script's own environment, not from the port).

## What Y2c / Y3 need

- **Y2c:** run the proofs' `--post` rows with YEBIS on too (`RENDER_ORACLE_YEBIS=1` as a row option).
  Put YEBIS on in the graphics preset and drop the stand-in once the rest is ported. Cover a second
  ppfilter, more than one frame (the luminance draw only shows from frame 2), and a window resize (the
  targets are made once, at start).
- **Y3:** eye adaptation (`UpdateAutoExposure`). Today exposure is the filter's maximum and the luminance
  target is drawn but never read back.
- **Not on wgpu / the web.** YEBIS needs Direct3D 11 bytecode, so the browser keeps the stand-in until
  the shaders have a route there (no translated shaders are shipped).

## Not done

- Glare, depth of field, rays of god and heat shimmer: `PostEffect::new` refuses a filter that turns them on.
- Only the first two frames' order is proven (the oracle logs one frame).

# Task Y2a: YEBIS's settings block and constant buffer, byte for byte

## Resume here

- **Both steps are done** (`crates/rustyac-render/src/yebis_cb.rs`). Next is Task Y2b: the device layer and
  the 8 draws, proven with log + pixels, then release v0.48.0.
- `PostEffect::new(ppfilter, video.ini, Camera)` builds the `CPostEffect` fields this chain reads, the way
  `CameraForwardYebis::setPostProcessing` 0x140025080 fills them. `round_two()` gives what
  `PFXALPHAOUT_EFFECTRESULT_ALPHA` forces in the second round.
- `Cbuffer::frame(pe, filmic, used)` returns one frame's 8 uploads (4096 bytes each). `Cbuffer` is YEBIS's
  global parameter store, so the store carries over between draws and frames. The game's logged frame is the
  second one from a fresh store.
- `filmic_table(acs.exe bytes)` reads the 21-float filmic curve from the player's `acs.exe` (file offset
  0x527cc0) at run time. It is not in the repo.
- Test: `cargo test -p rustyac-render --lib yebis_cb -- --nocapture`. It skips without an install or without
  the oracle logs in `re/scratch/taskY2a/runs/` (git-ignored). Those logs come from
  `RENDER_ORACLE_YEBIS=1 render_oracle run --side ac ... --post` (r23_09, r23_10, r23_16).
- No version, no tag (v0.48.0 comes with Y2b).

## What it found

1. **All 24 uploads are IDENTICAL** to the game's, byte for byte (3 scenes x 8 draws). The only bytes
   masked are the sign of draw 8's alpha (see question 2).
2. **The planted change is caught.** With `[COLOR] SATURATION` 0.95 -> 0.9, draw 4 (the tone map with the
   colour matrix) differs, and only draw 4.
3. **What an upload holds.** YEBIS zeroes 4096 bytes, then copies every variable that the draw's vertex and
   pixel programs mark used (`D3D_SVF_USED`, flag 2), from one global store. The copy runs to the end of
   the variable's register: the 8-byte `fParam_DepthCastScaleOffset` goes up with its register's .zw, which
   holds `+0x410`'s other half. Because nothing else is cleared, draws carry over values from earlier
   draws and from the last frame. For example, the CA's 5-tap weights show up in every later Modulate.
4. **What the frame's settings come to with `default.ini`:**
   - exposure is the auto-exposure MAX (0.5)
   - gamma 1.2
   - `FUNCTION=-1` becomes the filmic curve (`PFXTM_FILMIC`, id 10)
   - mapping 32
   - the colour matrix is Saturation(0.95) x ColorTemperature(6400) x WhiteBalance(6400)
   - CA 5 samples
   - the depth factor is (-64000, 0) for near 1, far 1000

   Round two forces exposure, gamma and mapping to 1, the tone map to `LINEARSAT` (id 1) and the colour
   matrix to the identity.
5. **The CA source rect is in texture coordinates** (1 x 1), not pixels. Its matrices depend on the
   aspect ratio and the dispersion only, not the render size.

| Scene | 1 depth | 2 CA | 3 luminance | 4 tone map | 5 depth | 6 CA | 7 tone map | 8 composite |
|---|---|---|---|---|---|---|---|---|
| r23_09 spa chase | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL |
| r23_10 spa chase MSAA 4 | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL |
| r23_16 monza chase | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL | IDENTICAL |

## What Y2b needs that this didn't do

- **Where the used ranges come from.** The test reads them from the real container with `rustyac-dxbc`.
  rustyac-dxbc is only an optional or dev dependency of rustyac-render, so the runtime path has to pick
  one: always link it, or carry the 7 techniques' ranges as a table.
- **A caller.** Nothing in `post.rs` or the oracle builds a `PostEffect` yet. `Camera` takes near, far,
  fov and size; the game's values come from `updateYebisParameters` 0x140029e30 (far clamped to [1, 1000]).
- **The device layer:** the state cache, the target pool, the ring offsets and the draws themselves.

## Not done

- **Settings this frame does not read (later tasks):** glare, DOF, light shafts, heat shimmer, lens
  distortion, feedback, vignette simulation, HUE and SEPIA, colour temperatures under 4000 K, and tone-map
  functions other than 0, 1 and filmic. `PostEffect::new` refuses these with an error rather than guessing.
- **Auto exposure over time (Y3).** Exposure is taken as settled at MAX, as in the oracle's frames (a
  `shortcut:` comment marks it).
- **No `run_all.sh`:** no proof area runs this code yet.

## Open questions

1. The CA weights and the XYZ->sRGB matrix are code immediates, written as `f32::from_bits` constants (a few
   dozen numbers, not code or bytecode). The filmic table is data in `.rdata` and is read at run time. Is that
   the right line, or should the immediates be read from `acs.exe` too?
2. Draw 8's alpha is `CRenderGlare+0x108` x 0 with the glare off. That field is never initialised, so the
   game uploads +0 or -0 by chance (-0 in r23_09, +0 in r23_10 and r23_16). The test masks that one sign bit;
   rustyAC writes +0. Y2b's pixel compare won't see it, but a strict log compare will.

# Task Y2: YEBIS's shader container read; the frame chain stopped at step 2 (rule 7)

## Resume here

- **Step 1 is done** (`crates/rustyac-render/src/yebis_tfxo.rs`). **Steps 2-4 were not started**: see "Why
  step 2 stopped". Super decides the split below (Y2a / Y2b / Y2c) before work goes on.
- `Tfxo::from_install(ac_root)` reads `acs.exe` from the player's install at run time and finds the
  container (`.tfxo file` magic, `hlsl` at +0x38). `technique("<name>")` gives the vertex and pixel programs
  of `tech_<name>`. Nothing of the container is in the repo, a release or the web pack.
- Test `cargo test -p rustyac-render --lib yebis_tfxo` (skips without an install): 2,649 programs, 2,653
  techniques, and the 7 techniques of the default frame hash to exactly the shaders the Y1 log shows bound.
- PPFX pseudo-C (git-ignored): 2,162 functions in `re/decomp/GPU*/`, `re/decomp/ppfx/`, `TinyEffect/`,
  indexed in `re/decomp/INDEX.tsv`. They come from `re/index/publics.tsv` (not `functions.tsv`), so plain
  `tools/bulk_decomp.py` finds none; the copy that works is `re/scratch/taskY2/decomp_ppfx.py --libs
  ppfx_dx11_x64.lib --match '^PPFX::'`.
- No version, no tag: the task is not finished.

## What Y2 found

1. **The 8 draws by name** (now in `docs/atlas/yebis.md` s8): 1 / 5 depth factor into the effect alpha
   (`MadAlphaColorT2_R_S1`, `DepthOfField_BlurFactorAlphaFromDepthTextureTypeCast...`); 2 / 6 camera
   motion blur (`TransformMadT1_S5`, a 5-tap zoom driven by the camera's movement since the last frame);
   3 the luminance measurement (`UpdateAutoExposure` -> `TonemapToSurface`, `TonemapHDR_Exposure`); 4 / 7
   the tone map (`DrawRectGPU_TonemapHDR` 0x1403f1b10, 36 parameters); 8
   `CompositeColorAndAlphaEffectsMask_A_A`.
2. **Y1 question 2, why two rounds:** AC initialises YEBIS with `PFXALPHAOUT_EFFECTRESULT_ALPHA`
   (`CameraForwardYebis::initPostProcessing` 0x140024ca0, also `YebisPP::initPP`). In that mode
   `EndPostEffectScene_Process` 0x1403b84d0 calls `EndPostEffectScene_InternalProcess` 0x1403b5f40 twice,
   the second time with exposure, colour matrix and related fields saved and forced to 1 / 0, and then
   composites colour and alpha mask (draw 8). It is not temporal. The two 1x1 textures are constants
   (black, grey 7f7f7f7f, white), not exposure history.
3. **Y1 question 3, the QUALITY rule:** `CameraForwardYebis::initRenderTargets` 0x140029a90 computes
   `(int)(QUALITY / 5.0 * width)`, 0 -> `min(width, 640)`, into `PFX_INITPARAM::m_uiEffectBufferCriterionWidth`
   (+0x3c). That only sizes the glare, DOF, motion-blur and AO sub-buffers (their `*WidthScale`). The effect
   buffer is `m_uiEffectBufferWidth` = 0 = the render size. So Y1's measurement (1280x720) is right and the
   map's section 3 wording was wrong; fixed.

## Why step 2 stopped (usage rule 7)

Step 2 asks for the parameter block and the 4 KB constant buffer value for value. Reading the code that
fills it shows it is not a block of maths but most of YEBIS's frame driver:
`EndPostEffectScene_InternalProcess` alone is 1,176 lines of pseudo-C, plus `Process`, `PreProcess`,
`UpdateParameters_Base`, `UpdateAutoExposure` with `CTextureLuminanceManage`, the depth-factor and
motion-blur passes, `TonemapToSurface` (2,958 bytes) and `DrawRectGPU_TonemapHDR`, the composite, and under
all of it the `IGPUDevice` state cache that decides which Direct3D calls are made, the render-target pool,
the vertex-buffer ring offsets and the constant-buffer leftovers, all of which carry over between frames.
About 50 KB of machine code. Step 4's tape scenes (r23_11..14) also need the camera motion and exposure
over several frames. That is a new subsystem, bigger than the 4-5 h Y1 guessed for the whole of Y2.

**Proposed split** (estimate 3-4 sessions in all):

| Task | Scope | Proof |
|---|---|---|
| **Y2a** | parameter block (video.ini + `ppfilters/default.ini` -> `CPostEffect`) and the per-draw constant buffer CPU maths | constant buffer bytes value for value against the Y1 log, single frames |
| **Y2b** | the device layer (state cache, target pool, ring offsets), the oracle `compare --post` with `RENDER_ORACLE_YEBIS` on both sides, the 8 draws | log + pixels IDENTICAL on r23_09, r23_10, r23_16; `run_all.sh --quick`; release v0.48.0 |
| **Y2c** (or folded into Y3) | camera motion and exposure over time for the tape scenes | r23_11..r23_14 |

**Plan Y3-Y7:** unchanged in order. Y3 shrinks: "the second round" is answered (not temporal), so Y3 is
auto exposure over time plus the VR / triple per-eye call. Total stays 7-9 tasks, Y2 now 2-3 of them.

## Not done

- Steps 2, 3 and 4: no parameter block, no constant buffer, no compare mode, no ported draw.
- No `run_all.sh --quick`: the only code is a reader with its own test, not on any proof path.
- Release v0.48.0 and its tag.

## Open questions

1. Super: take the Y2a / Y2b / Y2c split, or keep Y2 as one task and accept 3-4 sessions?
2. (From Y1, still open) does the page read the player's `acs.exe` already, or does the pack step need it?

# Task Y1: YEBIS mapped, a plan (the first pass is not ported yet)

## Resume here

- Steps 1 and 2 are done; **step 3 (port the first pass) was not started**, on purpose: see "Why step 3
  stopped". Super decides whether Y1's step 3 becomes Task Y2 as planned below.
- The map is `docs/atlas/yebis.md` (Task 23's `docs/map/yebis.md`, moved, with section 8 new: what one frame
  really draws, measured).
- New tool switch: `RENDER_ORACLE_YEBIS=1` on `render_oracle run --side ac ... --post` keeps the real YEBIS on
  the game's side; its Direct3D calls are in the `.gpulog`. Without it nothing changed (the stand-in copy).
- Raw logs (git-ignored): `re/scratch/taskY1/y1/` (real YEBIS), `y0/` (stand-in), `sum.awk` (one line per
  YEBIS draw: `awk -f re/scratch/taskY1/sum.awk <frame>.ac.gpulog`).
- No version, no tag: the task is not finished.

## What Y1 found

1. **The real YEBIS runs inside the oracle on WARP** and every call of it is logged. That makes YEBIS a normal
   rustyAC port job: log plus pixels against the game, the same proof as every renderer task since 20. Task
   23 thought it needed the two patches taken out "and compared against the real thing"; that part now works.
2. **With the oracle's profile (filter `default`, glare off, QUALITY 0) a frame is only 8 full-screen draws**,
   6 pixel shaders and 4 vertex shaders out of the container's 2,649, one 4096-byte constant buffer, a 128x64
   luminance target and full-size RGBA16F / RGBA8 targets. Table in `docs/atlas/yebis.md` section 8.
3. The pass order of `EndPostEffectScene_InternalProcess` from Task 23 is confirmed by the call index.
4. Correction to the Task 23 map: at QUALITY 0 the effect buffer is the full 1280x720, not 640 wide.

## Why step 3 stopped (usage rule 7)

"Port the first pass" cannot be proven alone: the game's picture is the whole 8-draw chain, so the smallest
pixel proof is all of it. That needs three things rustyAC does not have: a reader for YEBIS' shader container
in the player's `acs.exe`, the CPU side that fills the 4096-byte constant buffer per draw (tone-map,
exposure and colour-matrix maths from `CPostEffect`), and the oracle mode that compares both sides with
YEBIS on. Together that is a new subsystem of about one full task (desktop) plus a page part, not a step.

## The plan: Y2 ...

Each task ends with a picture proven against the game: `render_oracle compare ... --post` with
`RENDER_ORACLE_YEBIS=1` on the game's side, log IDENTICAL and pixels IDENTICAL, on the r23_09..r23_16 scenes
of `tools/proofs/render_list.txt` (spa, monza, imola, cockpit / chase / gate, MSAA 2 and 4, FXAA, tapes).

| Task | Scope | Size | Proof |
|---|---|---|---|
| **Y2** Container + the default chain, desktop | read the `.tfxo` container out of the player's `acs.exe` at run time (magic `.tfxo file`, `hlsl` at +0x38, `docs/atlas/yebis.md` s4); name the 10 logged programs; port the parameter block (video.ini + `ppfilters/default.ini` -> `CPostEffect` state) and the 8 draws with their constant buffers, exposure fixed by the filter; oracle: `compare --post` takes `RENDER_ORACLE_YEBIS` on both sides | 1 task (~4-5 h) | log + pixels, r23_09, r23_10, r23_16, one tape |
| **Y3** Auto exposure over time + the second round | the luminance read-back (one frame late), exposure adaptation, the 1x1 history textures, whatever round 5-8 is; the exposure apps and per-view exposure then change the picture | 1 task | 60-frame sequences, r23_11..r23_14 |
| ~~**Y4** The page~~ (dropped 2026-11-12: YEBIS stays desktop only; the browser draws without post-processing and its settings say "(desktop only)". Why: a scope choice, not something the game forces (so not a `differences.md` entry). YEBIS's code and bytecode never go into the web pack, so the page would have to read every program out of the player's `acs.exe` and translate it at load, and then every Y5-Y7 draw again on wgpu) | the same chain on wgpu: programs read from the player's `acs.exe` at load and translated by the existing DXBC path (`rustyac-dxbc`, `shader_table`, `shader_proof` on SwiftShader); `frameProof` with post on | 1 task | shader_proof on the 10 programs, page frame vs desktop |
| **Y5** Glare and light shafts | `GLARE` 1..5, `RAYS_OF_GOD`, `[GLARE]` / `[GODRAYS]` sections | 1-2 tasks | log + pixels with GLARE on, sun views |
| **Y6** Depth of field | `DOF`, `[DOF]`, auto focus, photo mode and the cinematic camera | 1-2 tasks | log + pixels with DOF on, photo views |
| **Y7** The rest | heat shimmer (volumes from `PostProcessEffectsUpdater`), lens distortion, chromatic aberration, vignette, film grain, switching filters at run time (`PostProcessFilterSelector`), every shipped `ppfilters/*.ini` once | 1 task | log + pixels per filter |

Total: 7-9 tasks. Desktop only (Y4, the page, dropped 2026-11-12).

**VR and triple screen.** `StereoCameraForward::renderYebis` 0x140213b10 calls `YebisPP::applyPostProcessing`
0x14002bb70, the same library with the same context, once per eye; `YebisPP::readPPSet` is a near copy of
`CameraForwardYebis::readPPSetOptions`. So the chain becomes one Rust function (source, depth,
destination, parameters) that `post.rs` calls once and the VR / triple cameras (`vr.rs`, `oculus.rs`,
`triple.rs`) call per eye / screen, plus a port of `YebisPP`'s own parameter code. It fits at the end of Y3
(after exposure, which is per camera) and is proven with the 47b/47c `--vr` / `--triple` rows.

**Where the page gets the shaders.** As for the game's own shaders since Task 38: never shipped, translated
in the page at load. The difference: they are inside `acs.exe`, not in `system/shaders`. The page needs the
container's 15 MB from the player's `acs.exe` (open question 1).

## Open questions

1. Does the page already have the player's `acs.exe` (in the content it reads from the install), or does the
   web pack step have to start carrying a read of it? This decides whether the preview pack gains a file.
2. Why YEBIS draws two rounds of 4 in one frame, and what its two 1x1 textures hold.
3. The QUALITY rule (section 8 of the map contradicts section 3).

## Not done

- Step 3: no YEBIS pass is ported, on the desktop or the page.
- No proof run (`run_all.sh --quick`): the only code change is an opt-in switch of the oracle; the default
  path was run once after the change (`re/scratch/taskY1/y0/`).
- Release v0.48.0 and its tag: left for when the first pass is in.
