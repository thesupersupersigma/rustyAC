# AC's renderer in the browser, part 1 (Task 38, v0.38.0)

## Resume here

Done: step 0 (the frame-rate check) and step 2 (the shader translator `crates/rustyac-dxbc` and
its proofs). Not started: step 1 (kgl behind one interface with a wgpu backend), step 3 (render
oracle sequences through native wgpu) and step 4 (the page's Full mode). Task 38b starts at step 1:
read "Not done" below first. The page still draws with its own debug renderer only, as in v0.37.2.

## In plain English

The browser game has to draw the track and cars with Assetto Corsa's own shaders, and a browser
cannot run them as they are: they are compiled Direct3D 11 programs. This task wrote a translator
that reads every compiled shader AC's renderer loads and writes it again in WGSL, the shading
language of WebGPU. All 72 shader pairs translate, and every result passes naga, the validator
wgpu uses. Run side by side with the originals on the same software rasterizer (WARP), 23 pairs
give the same bits in every pixel and 5 more are within 3 units in the last place; WARP's own
Direct3D 12 driver gave up on the rest, which run on the real graphics card. The translator also
reads the shaders' interface (constant buffers, variables, textures) and gives exactly what
Direct3D's `D3DReflect` gives, for all 144 files.

Before any of that, the page's frame loop was checked: a recorded race played in headless Chrome
at about 6 frames a second ends in the same state, step by step, and writes the same results file
as at 57 frames a second and as the desktop game.

Hooking the translated shaders into the renderer (a second backend under kgl) and into the page
did not fit in this task and goes to Task 38b.

## Step 0: the frame-rate check

The page's loop (`crates/rustyac-web/src/session.rs`) steps the simulation with a fixed step and an
accumulator; a frame longer than 0.1 s is cut there, so a slow page runs in slow motion, never with
other steps. Nothing in a step reads the clock. The difference Task 37c saw (a first Quick Race at
6 fps with other lap times) came from when race_out.json was taken: at the moment the test left the
drive, a wall-clock moment, by which a slow-motion race had driven less.

To prove it, the page got a check hook that does not depend on when anyone leaves:

- `Game::play_tape(race_ini, assists_ini, seed, tape)` plays a desktop `.ryin` tape from the
  session's first step and stops the frame loop at its end; `Game::stop_at_step(step)` stops an
  autodriven drive at a session step; `Game::check_state()` gives the step, whether it stopped,
  the state hash of every car chained over every step, and the player's car's state hash.
- `window.rustyac.frameProof(race, assists, tape64 | null, steps, slowMs)` (web/app.js) drives the
  race through the page's own `requestAnimationFrame` loop, every frame made at least `slowMs`
  long by busy work, and returns that state, race_out.json and the frame rate.
- `re/scratch/task38/frame_check.sh <name> <slow ms> [qr | rw | steps]` runs it in headless Chrome
  against `web/serve.py` with the test pack (2 cars, 2 tracks, the launcher theme).

| run | frames/s | steps | chain of every car's state | race_out.json |
|---|---|---|---|---|
| 37c Quick Race tape, full speed | 57.1 | 93,774 | `7aea29db0a574b43` | the same bytes as 37c's desktop run |
| the same tape, 160 ms busy per frame | 5.6 | 93,774 | `7aea29db0a574b43` | the same bytes |
| 37c's Chrome proof (`re/scratch/task37c/qr/chrome.json`) | (no frame loop) | 93,774 | `7aea29db0a574b43` | |
| autodriver (`?autodrive=1`) to step 30,000, full speed | 60.0 | 30,000 | car state at step 30,000 `3ace7ff0997081b6` | 548 bytes |
| the same, 160 ms busy per frame | 5.8 | 30,000 | car state at step 30,000 `3ace7ff0997081b6` | the same bytes |

(For the autodriver the chain itself starts when the test sets the stop, a wall-clock moment, so
only the car state at the fixed step is compared.)

## Step 2: the translator, `crates/rustyac-dxbc`

A small library with no dependencies (MIT OR Apache-2.0, like rustyac-math; it contains no shader):

- `parse(blob) -> Shader`: the DXBC container (chunks RDEF, ISGN, OSGN, SHDR; AC's files have no
  others), the reflection (`Reflection { constant_buffers, bindings }`, the same fields
  `D3DReflect` gives), the input and output signatures and the shader model 4 program as decoded
  instructions (`sm4.rs`).
- `translate_pair(vs, ps) -> Translated`: WGSL for both stages and what they bind. Registers are
  `vec4<u32>` bit patterns with a bitcast per instruction, so integer and float instructions see
  the same bits as on Direct3D; constant buffer `b<n>` is `@group(0) @binding(n)` as
  `array<vec4<u32>, N>`, texture `t<n>` `@group(1)`, sampler `s<n>` `@group(2)`; varyings are
  `@location(register)` with the pixel shader's interpolation copied onto the vertex outputs;
  slots read with a comparison sampler become depth textures.
- AC's 144 files: all shader model 4.1, about 55 opcodes, no `switch`, no indexable temps; seven
  carry an immediate constant buffer, 32 sample shadow maps with `sample_c_lz`, 12 loop.

It builds for `wasm32-unknown-unknown` (`cargo clippy -p rustyac-dxbc --target
wasm32-unknown-unknown`); it has not yet run there (38b, with the page).

### Proofs

- **Reflection**: `cargo test --release -p rustyac-render --test dxbc_reflect` reads all 144 files
  with `D3DReflect` and with `rustyac_dxbc::parse`: every constant buffer, variable (name, offset,
  size), bound resource and slot is the same. The D3D11 renderer keeps calling `D3DReflect`.
- **Translation**: `cargo run --release -p rustyac-dxbc --example shader_table -- "<AC>/system/shaders/win"`:
  72 of 72 pairs translate and pass naga's validator with every check on. `--dump <dir>` writes the
  WGSL for reading; it goes only into the git-ignored `re/scratch/task38/wgsl` and is never committed.
- **Pixels**: `cargo run --release -p rustyac-render --features wgpu --example shader_proof -- --adapter warp|gpu [name...]`
  draws one triangle over a 32×32 RGBA32F target per pair, once as AC's bytecode on Direct3D 11
  WARP and once as the WGSL on wgpu's Direct3D 12 backend, with the same inputs (a seed from the
  shader's name: identity matrices, other constants in 0..1, noise textures with all mips, noise
  depth maps) and compares every float bit for bit.

| | pairs |
|---|---|
| translate and pass naga | 72 of 72 |
| WARP against WARP: the same bits | 23 |
| WARP against WARP: within 3 ULP | 5 (ksIdealLine 2, ksMegaShader 3, ksPerPixelAT_NS 2, ksPerPixel_nosdw 2, ksTree 2) |
| WARP's Direct3D 12 driver lost the device (30) or crashed (6); all run on the GPU | 36 |
| not run: write SV_Depth (ksFXAA, ksPostBW, ksPostBlur, ksPostBlurRadial) or read multisampled textures (4) | 8 (the harness has no depth target and no MSAA inputs yet) |

On the real GPU (AMD Radeon Pro 5500M) 64 pairs run (all but the 8 the harness cannot feed); 12
give the same bits as Direct3D 11 WARP, the others differ as a GPU's texture filtering and maths
differ from WARP's, so that column says the translation runs there, not that it is exact.

The whole table:

| shader pair | WGSL valid (naga) | WARP D3D11 vs WARP wgpu | WARP D3D11 vs GPU wgpu |
|---|---|---|---|
| GL | yes | same bits | same bits |
| GL2D | yes | same bits | same bits |
| GLTextured | yes | same bits | 121/4096 same, ≤ 13479 ULP |
| ksBrakeDisc | yes | not run (WARP DX12 lost the device) | 1038/4096 same, ≤ 49291 ULP |
| ksBrokenGlass | yes | not run (WARP DX12 lost the device) | 47/4096 same, ≤ 26734 ULP |
| ksCameraDirt | yes | same bits | 172/4096 same, ≤ 3601 ULP |
| ksCarPaintSimple | yes | not run (WARP DX12 lost the device) | 41/4096 same, ≤ 9701 ULP |
| ksCircularRPM | yes | same bits | 100/4096 same, ≤ 7702 ULP |
| ksClouds | yes | same bits | 3092/4096 same, ≤ 5522 ULP |
| ksColourShader | yes | same bits | same bits |
| ksFakeCarShadows | yes | same bits | 3116/4096 same, ≤ 8637 ULP |
| ksFakeCarShadowsGen | yes | same bits | same bits |
| ksFlags | yes | not run (WARP DX12 lost the device) | 68/4096 same, ≤ 36695 ULP |
| ksFont | yes | same bits | 3099/4096 same, ≤ 15934 ULP |
| ksFXAA | yes | not run (writes depth) | not run (writes depth) |
| ksGrass | yes | not run (WARP DX12 lost the device) | 27/4096 same, ≤ 10285 ULP |
| ksHighPass | yes | same bits | 2377/4096 same, ≤ 7438336 ULP |
| ksIdealLine | yes | 3689/4096 same, ≤ 2 ULP | 117/4096 same, ≤ 4990 ULP |
| ksMegaShader | yes | 3005/4096 same, ≤ 3 ULP | 130/4096 same, ≤ 4047 ULP |
| ksMSDepthResolve | yes | not run (multisampled input) | not run (multisampled input) |
| ksMultilayer | yes | not run (WARP DX12 lost the device) | 1024/4096 same, ≤ 6447 ULP |
| ksMultilayer_fresnel_nm | yes | not run (WARP DX12 lost the device) | 1024/4096 same, ≤ 33602 ULP |
| ksMultilayer_objsp | yes | not run (WARP DX12 lost the device) | 1024/4096 same, ≤ 7724 ULP |
| ksOrenNayar | yes | not run (WARP DX12 lost the device) | 60/4096 same, ≤ 8840 ULP |
| ksParticle | yes | not run (WARP DX12 lost the device) | 1090/4096 same, ≤ 9320 ULP |
| ksPerPixel | yes | not run (WARP DX12 lost the device) | 103/4096 same, ≤ 6039 ULP |
| ksPerPixel_dual_layer | yes | not run (WARP DX12 lost the device) | 1027/4096 same, ≤ 2349 ULP |
| ksPerPixel_nosdw | yes | 3997/4096 same, ≤ 2 ULP | 165/4096 same, ≤ 6771 ULP |
| ksPerPixelAlpha | yes | not run (WARP DX12 lost the device) | 110/4096 same, ≤ 6954 ULP |
| ksPerPixelAT | yes | not run (WARP DX12 lost the device) | 148/4096 same, ≤ 13339 ULP |
| ksPerPixelAT_NM | yes | not run (WARP DX12 lost the device) | 48/4096 same, ≤ 12980 ULP |
| ksPerPixelAT_NS | yes | 3901/4096 same, ≤ 2 ULP | 106/4096 same, ≤ 8747 ULP |
| ksPerPixelMultiMap | yes | not run (WARP DX12 lost the device) | 1025/4096 same, ≤ 40004 ULP |
| ksPerPixelMultiMap_AT | yes | not run (WARP DX12 lost the device) | 25/4096 same, ≤ 10793 ULP |
| ksPerPixelMultiMap_AT_NMDetail | yes | not run (WARP DX12 lost the device) | 35/4096 same, ≤ 37196 ULP |
| ksPerPixelMultiMap_damage | yes | not run (WARP DX12 lost the device) | 1026/4096 same, ≤ 145820 ULP |
| ksPerPixelMultiMap_damage_dirt | yes | not run (WARP DX12 lost the device) | 1026/4096 same, ≤ 204316 ULP |
| ksPerPixelMultiMap_NMDetail | yes | not run (WARP DX12 lost the device) | 1025/4096 same, ≤ 164420 ULP |
| ksPerPixelMultiMapSimpleRefl | yes | not run (WARP DX12 lost the device) | 1026/4096 same, ≤ 4090 ULP |
| ksPerPixelNM | yes | not run (WARP DX12 lost the device) | 31/4096 same, ≤ 80068 ULP |
| ksPerPixelNM_UV2 | yes | not run (WARP DX12 lost the device) | 39/4096 same, ≤ 20270 ULP |
| ksPerPixelNM_UVMult | yes | not run (WARP DX12 lost the device) | 19/4096 same, ≤ 128209 ULP |
| ksPerPixelReflection | yes | not run (WARP DX12 lost the device) | 46/4096 same, ≤ 16962 ULP |
| ksPerPixelSimpleRefl | yes | not run (WARP DX12 lost the device) | 222/4096 same, ≤ 7967 ULP |
| ksPostAdaptLum | yes | same bits | 64/4096 same, ≤ 1925 ULP |
| ksPostBlur | yes | not run (writes depth) | not run (writes depth) |
| ksPostBlur_MS | yes | not run (multisampled input) | not run (multisampled input) |
| ksPostBlurH | yes | not run (WARP DX12 crashed) | 0/4096 same, ≤ 2308 ULP |
| ksPostBlurRadial | yes | not run (writes depth) | not run (writes depth) |
| ksPostBlurRadialMS | yes | not run (multisampled input) | not run (multisampled input) |
| ksPostBlurV | yes | not run (WARP DX12 crashed) | 1/4096 same, ≤ 1165 ULP |
| ksPostBW | yes | not run (writes depth) | not run (writes depth) |
| ksPostCopy | yes | same bits | 147/4096 same, ≤ 5911 ULP |
| ksPostCopyLuma | yes | same bits | 3080/4096 same, ≤ 2377 ULP |
| ksPostFOG | yes | not run (WARP DX12 crashed) | 1024/4096 same, ≤ 8870 ULP |
| ksPostFOG_MS | yes | not run (multisampled input) | not run (multisampled input) |
| ksPostToneMap | yes | same bits | 45/4096 same, ≤ 2271 ULP |
| ksSelectedMesh | yes | same bits | same bits |
| ksShadowGen | yes | same bits | same bits |
| ksShadowGen_debug | yes | same bits | same bits |
| ksShadowGenAT | yes | same bits | same bits |
| ksShadowGenSKIN | yes | not run (WARP DX12 crashed) | same bits |
| ksSimpleShader | yes | same bits | 1102/4096 same, ≤ 7453 ULP |
| ksSkidMark | yes | not run (WARP DX12 lost the device) | 101/4096 same, ≤ 52032 ULP |
| ksSkinnedMesh | yes | not run (WARP DX12 crashed) | 26/4096 same, ≤ 361812 ULP |
| ksSkinnedMesh_NMDetaill | yes | not run (WARP DX12 crashed) | 2/4096 same, ≤ 67353 ULP |
| ksSky | yes | same bits | same bits |
| ksSkyCubemap | yes | same bits | same bits |
| ksTest | yes | same bits | same bits |
| ksTree | yes | 3983/4096 same, ≤ 2 ULP | 140/4096 same, ≤ 12207 ULP |
| ksTyres | yes | not run (WARP DX12 lost the device) | 1031/4096 same, ≤ 19582 ULP |
| ksWindscreen | yes | not run (WARP DX12 lost the device) | 100/4096 same, ≤ 23887 ULP |

## Not done (Task 38b)

- Step 1: kgl behind one interface. Plan: a `wgpu` feature on rustyac-render that swaps `kgl.rs`
  for a `kgl_wgpu.rs` with the same methods, neutral type names so everything above kgl stays the
  same code, and the D3D-only device calls in `shader.rs`, `texture.rs`, `texture_fallback.rs`
  moved behind kgl methods. D3D11 path byte-unchanged: `kgl.rs` itself not edited.
- Step 3: render oracle sequences through native wgpu, kgl logs compared, PSNR and screenshots.
- Step 4: rustyac-render for wasm32, the page's Full mode, the mode switch, the fallback.
- The 36 pairs WARP's Direct3D 12 driver cannot run: find what it trips on (it is not the depth
  fill pass: a run without it fails the same way), or compare them on the GPU against Direct3D 11
  on the same GPU instead of WARP.
- The proof harness: a depth target for the 4 SV_Depth writers and multisampled inputs for 4.
- Running the translator in wasm (it builds; no run yet).

## Unchanged

`git diff v0.37.2 -- crates/rustyac-physics crates/rustyac-game crates/rustyac-net
crates/rustyac-render/src crates/rustyac-gui crates/rustyac-audio` is empty: the physics, AI, net,
game, GUI, sound and renderer sources are byte for byte v0.37.2's. The renderer crate gained only a
dev-dependency (rustyac-dxbc), an optional `wgpu` feature that nothing but the proof example turns on, a
test and an example. The page changed only by the check hooks above (`play_tape`, `stop_at_step`,
`check_state`, `frameProof`, `slowFrameMs`), which nothing calls unless a test does.

Run at the end (v0.38.0):

- `cargo test --release --locked -p rustyac-render`: every renderer golden (golden, golden_car,
  golden_cars, golden_leftovers, golden_post, golden_scene) passes on D3D11 WARP, plus the
  reflection test.
- `cargo test --release --locked -p rustyac-web -p rustyac-dxbc`: all pass.
- `cargo clippy --release --locked --all-targets -- -D warnings` for rustyac-render (with
  `--features wgpu`), rustyac-dxbc and rustyac-web on the desktop, and for rustyac-web
  (`--all-targets`) and rustyac-dxbc on `wasm32-unknown-unknown`.
- Every tool's lock file follows the new version (`cargo metadata --locked` on each).

Not rerun, because their code did not change (the diff above): the car oracle, the render
oracle sequences, the GUI sequences, the multiplayer tapes, the launcher answers and the physics
dumps against an old build.

## New or renamed files in the web zip

None. `web/build.py` builds the same file list as v0.37.2 (`app.js` and the wasm changed inside).

## Disk

36 GB free at the start, 27 GB at the end. Nothing deleted.

## Open questions (decided without asking)

- License of rustyac-dxbc: MIT OR Apache-2.0, as a small standalone library with no game code.
- The D3D11 renderer keeps `D3DReflect`; the reflection test proves the two agree, so the wgpu
  backend can use the parser alone.
- The frame-rate "fix" is a proof hook, not a change to the loop: the loop was already
  deterministic; race_out taken at a wall-clock moment is how a person leaves a race, and stays.
- Steps 1, 3 and 4 were left whole for 38b rather than half done, as the brief asks.
