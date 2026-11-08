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
| **Y4** The page | the same chain on wgpu: programs read from the player's `acs.exe` at load and translated by the existing DXBC path (`rustyac-dxbc`, `shader_table`, `shader_proof` on SwiftShader); `frameProof` with post on | 1 task | shader_proof on the 10 programs, page frame vs desktop |
| **Y5** Glare and light shafts | `GLARE` 1..5, `RAYS_OF_GOD`, `[GLARE]` / `[GODRAYS]` sections | 1-2 tasks | log + pixels with GLARE on, sun views |
| **Y6** Depth of field | `DOF`, `[DOF]`, auto focus, photo mode and the cinematic camera | 1-2 tasks | log + pixels with DOF on, photo views |
| **Y7** The rest | heat shimmer (volumes from `PostProcessEffectsUpdater`), lens distortion, chromatic aberration, vignette, film grain, switching filters at run time (`PostProcessFilterSelector`), every shipped `ppfilters/*.ini` once | 1 task | log + pixels per filter |

Total: 7-9 tasks. Desktop first in each, the page follows inside the same task from Y4 on.

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
