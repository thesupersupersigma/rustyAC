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
