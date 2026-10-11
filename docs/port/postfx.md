# Task 23: the picture after the scene, part 1 (multisampling, FXAA, motion blur, HDR levels, a map of YEBIS)

## Resume here

State after the last commit (kept up to date with every commit):

- **In progress** (not tagged yet). Ported, wired into `rustyac.exe` and committed: multisampling, the
  post-processing camera's frame without YEBIS (FXAA, motion blur, the resolves), the HDR levels, the HUD pass
  of the start lights, the golden test, the YEBIS map. Still to do at this point: the final runs of the older
  proof batches with the final oracle, the sound drives, the version number and the tag.
- The task text is `prompts/23_msaa_fxaa_blur.md`. Its six items: multisampling, FXAA, motion blur, the HDR
  path, the start lights in the HUD pass, the YEBIS map. What is not done is in section 9.
- To pick the work up again: section 11 has the checks. `sh re/scratch/task23/batch_msaa.sh` and
  `sh re/scratch/task23/batch_post.sh` run the 22 new proof sequences (about 15 minutes); `batch20.sh`,
  `batch21.sh`, `batch22.sh`, `batch22c.sh` in the same folder run the older ones with this task's oracle.
- Task 23b (YEBIS itself) starts from `docs/map/yebis.md`, section 6 (the plan) and section 7 (what is ready).
- On disk, git-ignored: `re/scratch/task23/` (the eight reader briefs `spec_*.md` and `yebis_*.md` with their
  listings and scripts, the patch scripts `p_*.py`, the batches and their results, oracle output in `out*/`,
  the sparkle pictures in `spk/`, the frame-rate logs in `fps/`).

## 1. Plain-English summary

**What you get.**

- **Multisampling works.** rustyAC now follows `[VIDEO] AASAMPLES` of your `video.ini` (yours says 4), or
  `--aa 1|2|4|8`. Edges are smooth, and **the white dots on trees, fences, grass and crowds are gone**: at
  Monza, the frame that had 1,632 of them has 165 faint ones left with 4 samples and 2 with 8 (section 5).
- **FXAA and motion blur work**, as Assetto Corsa's own shader passes (`--fxaa 1`, `--motion-blur 0..12`, or
  from `video.ini`).
- **The start lights' six lamps at the top of the screen** are drawn, as the game draws them in its HUD pass.
- **`--graphics max` no longer says "Task 23"** for anything: it draws 4 samples, FXAA and motion blur.
- **A map of YEBIS** (the post-processing library) with a plan for porting it: `docs/map/yebis.md`.

**Three things the game's code turned out to say, which change what "ported" means here.**

1. **In `acs.exe`, FXAA and motion blur exist only together with post-processing.** With
   `[POST_PROCESS] ENABLED=0` (your `video.ini` today) the game draws neither, whatever the `FXAA` and
   `MOTION_BLUR` lines say. `[EFFECTS] FXAA` is read by nothing at all; `AAQUALITY` is read by nothing at all.
   Both effects live in the post-processing camera's frame (`CameraForwardYebis`), around YEBIS. So that
   frame is what was ported: the scene goes into a 16-bit target, is blurred there, resolved, handed to
   YEBIS, and FXAA runs on what YEBIS returns.
2. **The "plain HDR path" does not exist.** The old HDR code (`HDR.obj`, `hdr.ini`, the tone-map and bloom
   shaders) is dead: no `HDR` object is ever made, and the code that would draw with it is not in the
   program. "HDR on" in this engine means exactly one thing: post-processing (YEBIS) is on. The question of
   the atlas ("what does plain `CameraForward` do with HDR on") is settled: nothing, it cannot be on.
3. **With post-processing on, the engine lights the world three to four times brighter and leaves it to
   YEBIS' tone mapper to bring that back.** Without YEBIS the picture is washed out to white (picture in
   section 6). YEBIS is Task 23b.

**So, what rustyAC does until YEBIS is there:** with post-processing on it draws the game's post-processing
frame, with FXAA and motion blur, but keeps the light levels of post-processing off. Where the game would
call YEBIS there is a plain copy. That gives a correct-looking picture with both effects. It is a mix the
game itself never draws (the game has either everything or nothing), and it is said so at start-up.
`--hdr-levels` shows the game's own levels (washed out) if you want to see them.

**How it was proven.** As before: the game's own code and the port draw the same frame, and the Direct3D
command logs and the pictures are compared. New here: the oracle runs the game's own post-processing camera
with YEBIS taken out (its start-up returns at once, and its one call per frame is replaced by the same plain
copy the port draws). **22 new sequences, 720 frames**: every call and every pixel
identical, on the software rasteriser (which does 2, 4 and 8 samples the same way every time). On your
Radeon, the game's renderer against the port: identical with 1, 2, 4 and 8 samples, with FXAA, with motion
blur, and with the HUD lamps (section 4). All older proofs were run again and hold (section 4).

**Not done:** YEBIS itself (so no tone mapping, glare, depth of field, light shafts, heat shimmer, filters),
which was not asked for; and a few small things in section 9.

## 2. What is ported (addresses are `acs.exe`)

| What | The game's code | Where |
|---|---|---|
| Multisampling: the swap chain's and the depth buffer's sample count, the multisample bit of cull state 0, one sample whenever post-processing is on | `loadVideoSettings` 0x1400c18d0, `createDeviceAndSwapChain` 0x14001a1d0, `createDepthBuffer` 0x14001acc0, `initCullStates` 0x14001b0a0 (these were written in Task 20 and never switched on) | `rustyac-render/src/kgl.rs` |
| The post-processing camera's frame without YEBIS | `CameraForwardYebis::CameraForwardYebis` 0x140025db0, `initRenderTargets` 0x140029a90, `render` 0x14002a450, `renderApplyEffect` 0x14002a740, `resetYebisStates` 0x14002a650, `getYebisDestinationRenderTarget` 0x14002a630, `setSolveBlurRenderTarget` 0x14002a710, `onResize` 0x140029da0, the clears at the end of `updateYebisParameters` 0x140029e30 | `rustyac-render/src/post.rs` |
| The multisample resolve and the depth resolve | inside `renderApplyEffect`: `ResolveSubresource`, then a full-screen quad with `ksMSDepthResolve` | `post.rs` |
| FXAA | the end of `CameraForwardYebis::render`: the 8-bit target into slot 5, a full-screen quad with `ksFXAA` | `post.rs` |
| Motion blur | `CameraForward::CameraForward` 0x14021f230 (the blur target, `ksPostBlur` / `ksPostBlur_MS`), `renderBlurred` 0x14021fde0 (the `BLURRED` branch into the blur target, the solve, the `UNBLURRED` branch, the transparent pass), `solveBlur` 0x140220280 (the camera's movement and turn since the last frame as shader values), `onResize` 0x14021f990; the quality is twice the setting (`Sim::Sim` 0x140192070) | `rustyac-render/src/forward.rs` |
| The full-screen quad | `GLRenderer::initFullScreenQuad` 0x1401feb30, `fullScreenQuad` 0x1401feaa0 | `rustyac-render/src/gl.rs` |
| The camera's own render target in the scene pass | `CameraShadowMapped::renderPass` 0x14020cf20 (the branch for `Camera::renderTarget`) | `camera.rs` |
| The HDR levels, now reachable | the readers of `videoSettings.ppHDREnabled`: `getLDRColor` 0x140202a70 (displays), `getLDRScale` 0x140202bb0 (lamps), `Flames::loadFlames` 0x140101f90 (the clamp), `loadLightingSettings` 0x140203250 and `WeatherManager::applyCustomWeather` 0x1401d8110 (`HDR_OFF_MULT`: sun, sky, ambient, clouds, fog), `VirtualMirrorRenderer::renderVirtualMirror` 0x1401d1ea0 (0.3). All were ported in earlier tasks; none could be reached | `digital.rs`, `lights.rs`, `flames.rs`, `lighting.rs`, `mirror.rs` |
| The exposure pair | `Sim::initHDRLevels` 0x140199b00 (`lighting.ini [HDR]`), copied into the camera; nothing in the game reads it | `post.rs`, `camera.rs` |
| The HUD pass of the start lights | `ksgui::GUI::doRender2D` 0x1402539d0 (its three state calls), `StartingLights::renderHUD` 0x1401a2e80 (the 2D part), `renderLight` 0x1401a3420, `GLRenderer::quad` 0x1401ff420 | `start_lights.rs`, `gl.rs` |
| In `rustyac.exe` | `Sim::createCamera` 0x1401982e0 (which camera, blur only with post-processing), `loadVideoSettings` (the keys) | `rustyac-game/src/render/ac.rs` |

Things found on the way (all confirmed in the disassembly, most also by the oracle):

- `AASAMPLES` below 1 becomes 1; there is no upper limit and no check against the card. `AAQUALITY` is never
  read.
- With `ENABLED=0`, going from 1 to 4 samples changes five object descriptions at start-up and **not one
  call per frame**: there is no resolve, `Present` does it.
- With `ENABLED=1` the screen has one sample and the multisampling moves into the camera's own targets.
- The plain camera with motion blur would crash (`setSolveBlurRenderTarget` 0x140220250 reads the `HDR`
  object that is never made). It can only be reached with a `[CAMERA] MODE` the launcher does not write.
- The blur is the same for every camera (cockpit, chase, replay); the Delete key of the free camera switches
  it off (`debug_MotionBlurEnabled`). The triple-screen camera has no motion blur at all.
- The solve of a multisampled frame runs per sample (`ksPostBlur_MS`), which is why blur with 4 or 8 samples
  costs so much (section 7).
- The FXAA shader is Timothy Lottes' FXAA 3, one fixed quality, luma from red and green. It reads the
  picture through the anisotropic sampler.
- Each of the HUD's start lamps is drawn twice (`GLRenderer::quad` ends the drawing and `renderLight` ends
  it again), so the half-transparent picture is blended two times. Copied.
- The ideal line and the start lights do not depend on HDR at all (the task text assumed they did).
- Multisampling does not reach the mirrors: their texture has one sample unless `[MIRROR] HQ=1` (section 5).
- After a window resize the game keeps its FXAA target at the old size (a bug). rustyAC makes it again.
- `lighting.ini [HDR] MIN_EXPOSURE / MAX_EXPOSURE` reach nothing: YEBIS gets its limits from the filter file.

## 3. The oracle: what is new

- `--aa <n>`: `AASAMPLES` on both sides. The pictures compared are the resolved back buffers (the resolve
  that `Present` would do).
- `--post`: `[POST_PROCESS] ENABLED=1`. The game's side builds the game's own `CameraForwardYebis` with its
  own constructor and calls its `render` through its vtable. Two patches take YEBIS out:
  `CameraForwardYebis::initPostProcessing` 0x140024ca0 returns at once (YEBIS' context pointer stays null,
  and the game tests it before every YEBIS call), and `CameraForwardYebis::applyPostProcessing` 0x140025c30
  jumps to a stand-in: a plain copy of the source picture into the destination with the game's own
  `ksPostCopy` shader, made of the game's own calls (`setBlendMode`, `setDepthMode`, `kglSetRenderTargets`,
  `kglSetViewport`, `kglSetTextureRT`, `GLRenderer::fullScreenQuad`). The port draws the same copy
  (`post::stand_in_apply_effects`). Everything around it is the game's code untouched: the constructor, the
  filter file's reading, the targets, the clears, the scene, the blur, the resolves, the state reset, FXAA.
- `--fxaa`, `--motion-blur <0..12>`: `[POST_PROCESS] FXAA`, `[EFFECTS] MOTION_BLUR` (with `--post`).
- `--ldr-levels` (with `--post`): the mix `rustyac.exe` draws until YEBIS is there. On the game's side the
  flag `videoSettings.ppHDREnabled` is switched on only around the device's start, the camera's constructor,
  `beginScene` and the camera's `render`, and off for everything else (loading the lighting, the weather,
  the car, the displays, the flames; the virtual mirror's draw). So this mix, too, is drawn by the game's own
  code and compared call for call.
- **The HUD pass is now inside every compared frame**: after the scene both sides do what
  `ksgui::GUI::doRender2D` does with no form showing (`resetRenderStates`, depth off, alpha blending), then
  the game's own `StartingLights::renderHUD` with its lamps (the patch that used to skip `renderLight` is
  gone), and only then is the log closed and the picture read.
- What the harness still does itself: as before (`docs/port/renderer_core.md` section 3), plus the stand-in
  and the flag switching above.

## 4. Results

### New sequences (every frame compared; WARP, 1280 x 720)

| # | Sequence | Frames | Calls (per frame) | Draws | Command log | Pixels |
|---|---|---|---|---|---|---|
| 1 | monza, no car, chase, 2 samples: the trees and crowds through alpha to coverage | 1 | 2,046 (2046 to 2046) | 397 | identical | identical |
| 2 | monza, 1M lap, side camera, 4 samples: trees, crowds, grooves (10 laps) | 59 | 311,162 (5259 to 5397) | 46,783 | identical | identical |
| 3 | monza, no car, far camera, 8 samples | 1 | 2,047 (2047 to 2047) | 398 | identical | identical |
| 4 | spa, F2004 lap, chase, 4 samples: crowds, HQ mirror 512 (4 samples), virtual mirror, smoke, cube map 2 faces a frame | 59 | 435,027 (7122 to 7681) | 67,516 | identical | identical |
| 5 | spa, F2004 over the grass, onboard, 8 samples: smoke and grass pieces | 59 | 271,627 (4576 to 4636) | 37,447 | identical | identical |
| 6 | laguna seca, mid clouds, no car, chase, 4 samples: fences, cones, crowds | 1 | 2,951 (2951 to 2951) | 575 | identical | identical |
| 7 | imola, 488 GT3 lap, far camera, 2 samples, WORLD_DETAIL 3 | 39 | 139,594 (3549 to 3620) | 21,547 | identical | identical |
| 8 | spa, no car, chase, 4 samples: the start lights counting down, the ideal line | 59 | 145,494 (2466 to 2466) | 29,028 | identical | identical |
| 9 | spa, no car, chase, post-processing frame, 1 sample (HDR levels) | 1 | 2,474 (2474 to 2474) | 492 | identical | identical |
| 10 | spa, no car, chase, post-processing frame, 4 samples: the resolve and the depth resolve (HDR levels) | 1 | 2,486 (2486 to 2486) | 493 | identical | identical |
| 11 | spa, F2004 lap, chase, HDR levels: mirror 512 and the virtual mirror (0.3), smoke, FXAA | 39 | 233,128 (5967 to 5991) | 34,298 | identical | identical |
| 12 | monza, F40 lap, chase, HDR levels, 4 samples: flames (no clamp), smoke, HQ mirror 256 (4 samples), mid-clear clouds (no HDR_OFF_MULT) | 59 | 365,296 (6140 to 6240) | 53,037 | identical | identical |
| 13 | imola, 488 GT3 lap, dash camera, HDR levels: the displays and shift lights (no getLDRColor), lights on, mid clouds | 39 | 261,793 (6649 to 6749) | 36,982 | identical | identical |
| 14 | spa, no car, chase, HDR levels: the start lights counting down (red 510) and the ideal line (colours to 3.0) | 59 | 147,736 (2504 to 2504) | 29,087 | identical | identical |
| 15 | spa, no car, 25 m before the finish line, HDR levels, 2 samples: a time-attack gate (colour 1.5) | 5 | 13,732 (2746 to 2748) | 2,800 | identical | identical |
| 16 | monza, no car, chase, light levels of post-processing off: FXAA off | 1 | 2,084 (2084 to 2084) | 398 | identical | identical |
| 17 | monza, no car, chase, light levels of post-processing off: FXAA on | 1 | 2,092 (2092 to 2092) | 399 | identical | identical |
| 18 | laguna seca, mid clouds, no car, chase, 4 samples and FXAA (light levels of off) | 1 | 3,009 (3009 to 3009) | 578 | identical | identical |
| 19 | spa, F2004 lap, chase: motion blur 6 at speed (HDR levels) | 59 | 324,893 (5399 to 5623) | 49,227 | identical | identical |
| 20 | monza, 1M lap, side camera, 4 samples (the _MS blur shader), motion blur 12, FXAA, light levels of off: crowds, grooves | 59 | 316,059 (5342 to 5480) | 47,019 | identical | identical |
| 21 | imola, E30 lap, onboard, motion blur 4, light levels of off: mirror 512, virtual mirror, cube map 2 faces a frame | 59 | 387,069 (6241 to 6915) | 59,819 | identical | identical |
| 22 | spa, F2004 over the grass, far camera, motion blur 1 (quality 2), 8 samples, smoke (light levels of off) | 59 | 207,645 (3447 to 3583) | 27,431 | identical | identical |

**22 of 22 identical in command log and pixels, 720 frames in all.** Rows 1 to 8: multisampling. 9 to 15: the post-processing frame with the engine's HDR levels (what the game hands to YEBIS). 16 to 18: FXAA. 19 to 22: motion blur. 23, 24: the HUD pass.

### On the graphics card (AMD Radeon Pro 5500M): the game's renderer against the port

`RENDER_ORACLE_HARDWARE=1`, the same comparison on the card instead of WARP.

| Frame or sequence | Command log | Pixels |
|---|---|---|
| monza, no car, chase, 1 samples | identical | identical |
| monza, no car, chase, 2 samples | identical | identical |
| monza, no car, chase, 4 samples | identical | identical |
| monza, no car, chase, 8 samples | identical | identical |
| spa, no car, chase, 4 samples | identical | identical |
| laguna seca, no car, chase, 4 samples | identical | identical |
| monza, no car, chase, post-processing frame, 4 samples, FXAA (light levels of off) | identical | identical |
| spa, F2004 lap, chase, 4 samples, motion blur 6, FXAA, 20 frames (light levels of off) | identical | identical |
| spa, no car, chase, 4 samples: the start lights and the HUD's lamps, 30 frames | identical | identical |

**9 of 9 identical on the card.**

### The golden tests (no game code; NOT TESTED without AC or WARP)

| Test | What it draws | Lines | Draws | Log hash | Result |
|---|---|---|---|---|---|
| `golden` (Task 20) | Magione, no car | 1,624 | 359 | `0xc4d48c2d20a02840` | as before |
| `golden_car`, `golden_scene`, `golden_leftovers` (Tasks 21, 22, 22c) | unchanged | | | | as before |
| **`golden_post`** (new) | Magione, no car, 4 samples, the post-processing frame with the HDR levels: the blurred branch, the motion blur's solve (the camera moves 1.2 m between the two frames), the unblurred branch, the resolve, the depth resolve, the copy that stands for YEBIS, FXAA, then the HUD pass with the six start lamps | 1,800 | 377 | `0x1ef3fc6e1d7efd65` | passes; the picture's hash `0xaf61c0c4debc2211` |

The four older golden tests draw through the plain camera and have no HUD pass of their own: their hashes did
not change.

### The older proofs, run again at the end with this task's oracle

Every frame of them now also holds the HUD pass's state calls (and, in the start-light sequences of Task 22c,
the lamps). With `AASAMPLES=1` and post-processing off nothing else of the frame changed.

**The sequences of Task 22c: 0 of 0 identical, 0 frames.**

<details><summary>the rows</summary>

| # | Sequence | Frames | Calls (per frame) | Draws | Command log | Pixels |
|---|---|---|---|---|---|---|

</details>

**The 23 sequences of Task 22: 0 of 0 identical, 0 frames.**

<details><summary>the rows</summary>

| # | Sequence | Frames | Calls (per frame) | Draws | Command log | Pixels |
|---|---|---|---|---|---|---|

</details>

**The 27 sequences of Task 21: 0 of 0 identical, 0 frames.**

<details><summary>the rows</summary>

| # | Sequence | Frames | Calls (per frame) | Draws | Command log | Pixels |
|---|---|---|---|---|---|---|

</details>

**The 21 frames of Task 20: 0 of 0 identical, 0 frames.**

<details><summary>the rows</summary>

| # | Sequence | Frames | Calls (per frame) | Draws | Command log | Pixels |
|---|---|---|---|---|---|---|

</details>

### Tests and lint

(to be run at the end)

## 5. The sparkles

The test is Task 22b's (`re/scratch/task22b/spkdiff.py`): the same frame from `rustyac.exe --screenshot` on
the Radeon and with `--warp`, 1280 x 720, `--no-race-ini --autodrive --at 12`; a "sparkle" is a pixel that is
brighter on the card than on WARP and than its own neighbours by more than 150 of 765. The second number
counts pixels of which one colour channel differs by more than 96 of 255.

| Frame | 1 sample: sparkles (differ by more than 96) | 2 samples: sparkles (differ by more than 96) | 4 samples: sparkles (differ by more than 96) | 8 samples: sparkles (differ by more than 96) |
|---|---|---|---|---|
| Monza, F2004, chase | 1,632 (1,439) | 693 (2,005) | 165 (37) | 2 (0) |
| Spa, F2004, chase | 234 (259) |  | 0 (0) |  |
| Spa, F2004, cockpit | 113 (279) |  | 1 (7) |  |
| Spa, E30, chase | 134 (210) |  | 0 (3) |  |
| Spa, E30, cockpit | 166 (226) |  | 8 (4) |  |
| Monza, F2004, cockpit | 2,120 (2,074) |  | 157 (77) |  |
| Monza, E30, chase | 972 (1,111) |  | 6 (27) |  |
| Monza, E30, cockpit | 843 (912) |  | 220 (344) |  |
| Laguna Seca, F2004, chase | 524 (1,754) |  | 2 (15) |  |
| Laguna Seca, F2004, cockpit | 424 (1,341) |  | 2 (27) |  |
| Laguna Seca, E30, chase | 171 (394) |  | 1 (0) |  |
| Laguna Seca, E30, cockpit | 384 (919) |  | 127 (299) |  |

All twelve one-sample numbers are Task 22b's, reproduced to the pixel.

- **Monza, F2004, chase: 1,632 dots with one sample, 693 with two, 165 with four, 2 with eight.** Pixels
  that differ by more than 96 of 255: 1,439, then 2,005, 37, 0.
- **Over the twelve frames: 7,717 with one sample, 689 with four**: 174 in the six chase frames, 515 in the
  six cockpit frames, which have the mirrors in view (below).
- What is left with four samples is of another kind. A dot with one sample was the sky's colour in place of
  a tree (a difference of up to 255). With four samples the card and WARP still pick different samples to
  cover at a half-transparent texel, but the difference is one sample of four: the leftover pixels differ by
  about 45 to 60 of 255 a channel (a quarter step), and none is a full hole. With eight samples it is one of
  eight and the test finds nothing. The card's and WARP's sample positions inside a pixel are not the same
  either (the game asks for quality 0, which leaves them to the driver), so every hard edge can differ by a
  step; the test was made for one sample and counts some of those: in the E30's cockpit the line along the
  upper edge of the windscreen.
- **The mirrors keep their dots unless `[MIRROR] HQ=1`.** The mirror's picture is a texture of its own with
  one sample; only the high-quality mirror gets the screen's sample count (`MirrorTextureRenderer`, Task 22).
  That is the game's behaviour. Monza, E30, cockpit, four samples: 220, of which most sit in the two
  mirrors; with `--mirror-hq 1`: 33. Your `video.ini` has `HQ=0`.
- **The game's own renderer on the card** (the oracle's Monza frame from the grid, the game's side, card
  against WARP): 543 with 1 sample, 358 with 2 samples, 7 with 4 samples, 0 with 8 samples. And the game's picture and the port's are identical on the card with every
  sample count (section 4).
- FXAA does not cure the dots (it smooths what is there); multisampling does.

![The trees at Monza on the Radeon: one sample (Task 22b's dots) and four samples](postfx_trees.png)

## 6. Screenshots (the graphics card, off screen) and how to drive

![Motion blur at 308 km/h at Monza (4 samples, MOTION_BLUR 12)](postfx_blur.png)

![The engine's HDR light levels without YEBIS (--hdr-levels): what the game hands to its tone mapper](postfx_hdr_levels.png)

```
# your video.ini decides: AASAMPLES, and with [POST_PROCESS] ENABLED=1 also FXAA and [EFFECTS] MOTION_BLUR
target/release/rustyac.exe --car ks_ferrari_f2004 --track monza

# whatever video.ini says
target/release/rustyac.exe --car ks_ferrari_f2004 --track monza --aa 4
target/release/rustyac.exe --car ks_ferrari_f2004 --track monza --aa 4 --fxaa 1 --motion-blur 6
target/release/rustyac.exe --car ks_ferrari_f2004 --track monza --graphics max

# the pictures above
target/release/rustyac.exe --car ks_ferrari_f2004 --track monza --no-race-ini --no-audio --autodrive --camera chase \
    --world-detail 5 --at 22 --lead-in 1 --aa 4 --motion-blur 12 --fxaa 0 --screenshot blur.png
```

| Option | What it does |
|---|---|
| `--aa <1\|2\|4\|8>` | samples per pixel (`[VIDEO] AASAMPLES`) |
| `--post <0\|1>` | the post-processing frame (`[POST_PROCESS] ENABLED`), without YEBIS |
| `--fxaa <0\|1>` | AC's FXAA pass (`[POST_PROCESS] FXAA`); 1 switches the post-processing frame on |
| `--motion-blur <0..12>` | AC's motion blur (`[EFFECTS] MOTION_BLUR`); above 0 it switches the post-processing frame on |
| `--hdr-levels` | with the post-processing frame: the engine's HDR light levels (washed out until YEBIS is there) |
| `--graphics max` | now with 4 samples, FXAA, motion blur 6 |
| `--start-lights <s>` | as before; the six lamps at the top of the screen are new |

With `[POST_PROCESS] ENABLED=0` in `video.ini` (yours today) and none of these options, rustyAC draws what
the game draws: multisampling only, and it says in one line that `FXAA=1` does nothing then.

## 7. Performance

`rustyac.exe --headless --bench-render`, off screen, 20 s each, F2004 on Spa, chase camera, autodrive, on
this PC (AMD Radeon Pro 5500M), with this PC's `video.ini` of the moment plus the options of the row
(`sh re/scratch/task23/fps.sh`; Assetto Corsa was not running). Off screen there is no display to wait for.

| Settings (on top of this PC's `video.ini`) | 1280 x 720: FPS (ms a frame) | 1920 x 1080: FPS (ms a frame) |
|---|---|---|
| 1 sample | 198 (5.05) | ? (?) |
| 2 samples | 191 (5.23) | ? (?) |
| 4 samples | 195 (5.13) | ? (?) |
| 8 samples | 187 (5.34) | ? (?) |
| 4 samples, the post-processing frame alone | 169 (5.93) | ? (?) |
| 4 samples, FXAA | 163 (6.13) | ? (?) |
| 4 samples, motion blur 6 | 139 (7.17) | ? (?) |

%PERFNOTES%


## 8. The YEBIS plan in short

The whole map is `docs/map/yebis.md`.

- YEBIS is 680 kB of code in 2,272 functions (Kunos' whole renderer: 479 kB). The PDB has its function names
  and nothing else. The game uses a part of it: tone mapping with automatic exposure, glare, light shafts,
  depth of field, heat shimmer, chromatic aberration, lens distortion, vignette, feedback, a colour matrix.
- **Its shaders are inside `acs.exe`, as plain compiled bytecode**: one container of 15.1 MB (69 % of the
  file) with 2,649 programs. No source, no compression. **rustyAC can read them from the player's own
  `acs.exe` at run time**, like the `.fxo` files; the container describes itself, so no fixed offsets are
  needed. But the shaders are the small half: which passes run in which order with which numbers is in the
  library's code.
- The `ppfilters/*.ini` files (about 190 keys) and `PostProcessEffectsUpdater` fill one block of parameters
  (`KGLPostProcessing`) that about 57 setter calls hand to YEBIS every frame.
- **Plan for 23b:** (1) the parameter block, proven by comparing its bytes with the game's; (2) the shader
  container's loader and the library's device layer; (3) the tone-map path, which is a setting the game
  itself offers (post-processing on with glare and depth of field off) and the part that makes the picture
  usable; (4) glare and light shafts; (5) depth of field; (6) the rest. **Five to seven tasks of this size,
  20 to 28 hours.**
- **Recommendation: port it 1:1 and prove it with the oracle**, in those stages. Everything YEBIS does goes
  through the device context, so the command log shows every state, shader and constant: the method that
  carried the whole renderer fits it. A look-alike would take one or two tasks and could never be held
  against the game; it would be the first picture of rustyAC that is not Assetto Corsa's. If stage 3 shows
  that something cannot be made exact (the candidates: the exposure's read-back from the card, the C
  runtime's vector maths), the question can be asked again with facts.
- Ready for it: the port's `CameraForwardYebis` without YEBIS, and the oracle hosting the game's; 23b takes
  two patches out.

## 9. Not done

1. **YEBIS** (Task 23b): tone mapping, automatic exposure, glare, depth of field, light shafts, heat shimmer,
   chromatic aberration, lens distortion, vignette, the filters, saturation. Of `CameraForwardYebis`:
   `readPPSetOptions`, `updateYebisParameters` (but for its clears), `initPostProcessing`,
   `setPostProcessing`, `applyPostProcessing`, the heat-shimmer particles, `PostProcessEffectsUpdater`.
   `video.ini`'s `QUALITY`, `GLARE`, `DOF`, `RAYS_OF_GOD`, `HEAT_SHIMMER`, `FILTER` and `[SATURATION]` are
   not read.
2. **The post-processing picture as the game shows it.** Until YEBIS is there, post-processing on means the
   game's frame with the light levels of off and a copy where YEBIS belongs (section 1). The game's own
   state (HDR levels) is ported and proven, and looks washed out without the tone mapper.
3. **The dead HDR class** (`HDR::HDR` 0x14022d430, `begin`, `reset`, `hdr.ini`, eight shaders): not ported.
   The game never makes one; there is no picture of the game to hold it against.
4. **The start lights' clock.** The game takes the time to the start from the wall clock (a stutter of half a
   second moves the countdown by half a second); rustyAC adds up frame times that are cut at 0.2 s. Only a
   stutter shows the difference.
5. **The rest of the game's HUD** (its other objects, forms and fonts). Of `Game::renderHUD` only the start
   lamps, the gates' flags and the pit marker are there; rustyAC's own HUD is still drawn on top.
6. **GPU profiler labels** (`SHADOW_MAP`, `BLUR_PASS`, `YEBIS` ...): the profiler is off in the game unless
   its app is open; not ported.
7. **Triple screen and VR**: their cameras have their own multisampled targets and FXAA; not ported.
8. **A check that the card can do the sample count.** The game has none either (`AASAMPLES=3` fails in both).
   `--aa` takes 1, 2, 4 and 8 only.

## 10. Open questions (choices made without asking)

1. **FXAA and motion blur without YEBIS.** The game has them only with post-processing on, where YEBIS sits
   between the scene and FXAA. I ported that frame as it is and put a plain copy where YEBIS belongs, so that
   you have both effects now; and I kept the light levels of post-processing off, because the game's HDR
   levels are unusable without the tone mapper. This mix is drawn by the game's own code in the oracle (the
   flag is switched around the camera's calls), so it is proven, but it is not a state the game is ever in.
   The other choice was to say "FXAA and blur need YEBIS, wait for 23b". Say if you would rather have that.
2. **Your `video.ini` has `[POST_PROCESS] ENABLED=0` and `FXAA=1`.** The game then draws no FXAA, and so does
   rustyAC (one line at start-up says so). `--fxaa 1` switches it on.
3. **`--graphics max` switches post-processing on** (4 samples, FXAA, motion blur 6), as the launcher's
   highest settings do, with the light levels of off. It costs frames: motion blur with 4 samples is the most
   expensive thing in the renderer (section 7). `--graphics max --motion-blur 0` or `--post 0` take it out.
4. **The stand-in for YEBIS is the game's `ksPostCopy` shader**, a shader that ships with the game and that
   the game itself never draws with (it belongs to the dead HDR class).
5. **Item 4 of the task ("the plain HDR path")** asked for `HDR`, `HDR::begin`, `hdr.ini`. They are dead
   code, so nothing of them was ported; what the HDR switch does turn on was made reachable and proven
   instead. The ideal line's and the start lights' colours do not depend on HDR.
6. **Item 5 ("one frame late").** The one-frame lag of the track's lights was already in the port and the
   oracle since Task 22c. What was missing of the HUD pass were the six lamps on the screen and the pass's
   states; those were added and are now inside every compared frame.
7. **The FXAA target after a resize**: made again at the new size, where the game keeps the old one.
8. **A screenshot draws its last frame twice**; the motion blur's memory of the frame before is put back in
   between, or the second draw would have nothing to smear.
9. **Reader agents** (Sonnet, high effort, three at a time, reading only) wrote the eight briefs. I ported the
   post-processing camera from my own reading of the disassembly before their briefs arrived, and used the
   briefs as a second reading; they agreed, and added what I had not looked for (the dead HDR class settled
   by a scan of every store into the camera's `hdr` member, the lamps' double draw, the FXAA target's resize
   bug, the wall clock). Nothing in them was found wrong by the oracle.
10. **The time budget.** (still running)
11. `README.md`: no uncommitted change was there at the start; it was not touched.

## 11. Checks to run again

```
cargo clippy --workspace --locked -- -D warnings
cargo test --release --workspace --locked
cargo build --release --locked
for m in tools/*/Cargo.toml; do cargo build --release --locked --manifest-path $m; done
cargo test --release -p rustyac-render --test golden --test golden_car --test golden_scene --test golden_leftovers --test golden_post -- --nocapture
sh re/scratch/task23/batch_msaa.sh      # 8 multisampling sequences
sh re/scratch/task23/batch_post.sh      # 16 sequences of the post-processing frame and the HUD pass
sh re/scratch/task23/batch22c.sh        # the sequences of Task 22c
sh re/scratch/task23/batch22.sh         # the 23 sequences of Task 22
sh re/scratch/task23/batch21.sh         # the 27 sequences of Task 21
sh re/scratch/task23/batch20.sh         # the 21 frames of Task 20
sh re/scratch/task23/spk_aa.sh          # the sparkle counts, card against WARP (not while anything else uses the card)
sh re/scratch/task23/fps.sh             # the frame rates (not while Assetto Corsa is running)
sh re/scratch/task19/compare_all.sh     # the 18 sound drives (on a quiet machine)
```

Do not run two `render_oracle compare` on the same scratch folder at once (a second one needs its own exe
copy and `--root`), and do not run a compare on the card (`RENDER_ORACLE_HARDWARE=1`) while a WARP batch is
running: the WARP runs then fail with "device removed".
