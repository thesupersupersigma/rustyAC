# AC's renderer in the browser, part 1 (Tasks 38 and 38b, v0.38.0 and v0.38.1)

## Resume here

Task 38b (v0.38.1) did step 1: every one of the 72 shader pairs is now compared on a software
rasterizer (SwiftShader through Vulkan, since WARP's Direct3D 12 driver cannot run 37 of them),
40 with the same bits and 32 within 1–51 ULP, each difference traced to its cause. Not started:
step 2 (kgl behind one interface with a wgpu backend), step 3 (render oracle sequences through
native wgpu) and step 4 (the page's Full mode): Task 38c starts at step 2, read "Steps 2–4: not
done (Task 38c)" in the Task 38b part at the end first. The page still draws with its debug
renderer only. Step 5 (every end-to-end check) passed except a fresh `rc_jump` recording: see
the Task 38b open questions.

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

# Task 38b (v0.38.1)

## In plain English (38b)

Every one of AC's 72 shader pairs has now been drawn by its WGSL translation on a software
rasterizer and compared pixel by pixel with AC's original bytecode on Direct3D 11's WARP. WARP's own
Direct3D 12 driver could not run 37 of them, for reasons that are WARP's and not the translation's
(below), so a second software rasterizer was added: SwiftShader, Google's CPU Vulkan driver (the one
Chrome uses when it has no graphics card). On SwiftShader all 72 pairs run: 40 give the same bits
in every pixel, the other 32 differ by 1 to 51 units in the last place, and each of those is traced
to one cause: SwiftShader computes `exp2`/`log2`/`sin`/`cos` a little differently from WARP (27
pairs), or a chain of multiplies is regrouped by the compiler under wgpu (5 pairs, the same on both
software rasterizers). No pair differs because of the translation.

The renderer itself (kgl behind one interface, a wgpu backend, the render oracle through it and the
page's Full mode, steps 2–4 of this task) did not fit and goes whole to Task 38c. The page still
draws with its debug renderer only. Nothing the game, the oracles or the goldens use changed; the
full end-to-end checks were run (step 5) and pass, all but one: a jump-start race recorded afresh
from the game no longer matches the port, while Task 28's saved recordings of it still do (the
port's code has not changed since v0.37.2; see the open questions).

## Step 1: every shader on a software rasterizer

### Why WARP's Direct3D 12 driver fails on 37 pairs

- **Lost device on every pair that samples a shadow map** (30 pairs: every `ksPerPixel*` with
  shadows, `ksMultilayer*`, `ksBrakeDisc`, `ksTyres`, ...). Narrowed down with hand-written pixel
  shaders: a constant runs, plain `textureSample` and `textureLoad` of the same depth textures run,
  a single `textureSampleCompareLevel(t, s, uv, 0.5)` loses the device. Still lost without the
  depth fill pass, with `Depth16Unorm`, without the render-attachment usage, with a nearest compare
  filter, and with DXC in place of FXC (the 10.0.26100 DXC; 19041's lacks `IDxcCompiler3`). The
  D3D12 debug layer and GPU-based validation say nothing before `DXGI_ERROR_DEVICE_REMOVED`; wgpu's
  last message is `Buffer view error: Validation Error`. The same WGSL runs on the graphics card and
  on SwiftShader, and gives the same bits as Direct3D 11 there, so it is WARP D3D12 with wgpu's
  comparison-sampler path (naga's HLSL puts comparison samplers in a separate sampler heap array),
  not our translation.
- **Process crash (access violation) on every pair with a loop** (ksPostBlurH, ksPostBlurV,
  ksPostFOG, ksPostFOG_MS, ksShadowGenSKIN, ksSkinnedMesh, ksSkinnedMesh_NMDetaill: 7 pairs): WARP D3D12's shader compiler crashes on the `loop {}` naga
  writes. Same answer on the GPU and SwiftShader.

### The second software reference: SwiftShader

`--adapter swiftshader` opens wgpu's Vulkan backend on SwiftShader's `vk_swiftshader.dll`, found
through `VK_DRIVER_FILES`. The DLL and its ICD json are copied from Chrome's own folder into the
git-ignored `re/scratch/task38b/swiftshader/`; they are not in the repository or any release.
wgpu's renderer feature gained `vulkan` (and with it the crates `ash` and `spirv`, in
LICENSING.md); only the proof example turns the feature on.

### What the harness gained

- **Depth writers** (ksFXAA, ksPostBW, ksPostBlur, ksPostBlurRadial): a 32-bit float depth target,
  test ALWAYS, write on, cleared to 0, read back and compared like the colour targets.
- **Multisampled inputs** (ksMSDepthResolve, ksPostBlur_MS, ksPostBlurRadialMS, ksPostFOG_MS):
  4-sample RGBA16F textures filled by a small pass of exact numbers (an integer formula of x, y
  and the sample index, divided by 64), the same pass in HLSL and WGSL.
- **`--flat`**: every texture one value (a half float, so the conversion is exact) on every level.
  Texture filtering is not the same on two rasterizers (the noise runs differ by up to 12M ULP on
  every textured pair) and is not part of the translation; flat textures take it out.
- **`--ops`**: eleven single instructions alone (exp2, log2, rsqrt, sqrt, div, sin, cos, mad,
  dp4, ...) over 4096 inputs, to tell the rasterizer's maths from the translation.
- Knobs for narrowing a difference down: `PROOF_DIFF=1`, `PROOF_VALUES=1`, `PROOF_DUMP=1` (the
  WGSL to stdout, never to a file), `PROOF_PS_HLSL=<file>` (one HLSL pixel shader on both sides),
  `PROOF_PS`/`PROOF_VS=<file>`, `PROOF_LOG=1`. All in the example's header.

### Results

| | SwiftShader, flat textures | WARP D3D12, flat textures |
|---|---|---|
| the same bits | **40** | 30 |
| differ (largest) | 32 (1–51 ULP) | 5 (2–3 ULP) |
| not run | **0** | 37 (30 lost device, 7 crash) |

Single instructions against WARP D3D11:

| instruction | SwiftShader: largest ULP (same of 4096) | WARP D3D12 |
|---|---|---|
| mad, dp4, mul+add, rsqrt, sqrt, div | 0 (4096) | 0 |
| exp2 | 4 (1168) | 0 |
| log2 | 176 (3309) | 0 |
| sin | 42002 (1), near its zeros | 0 |
| cos | 80400 (1), near its zeros | 0 |

So:

- **SwiftShader's maths** (27 pairs, 1–51 ULP): every one calls `exp2` and most `log2` (`pow` in
  the HLSL), ksFlags also `sin`. ksPostAdaptLum (1 ULP) shows it alone: it gives the same bits on
  WARP D3D12, whose exp2 is WARP's.
- **A multiply chain regrouped under wgpu** (ksIdealLine 2, ksMegaShader 3/4, ksPerPixelAT_NS
  3/2, ksPerPixel_nosdw 2, ksTree 2/4 ULP; on both rasterizers). Proven on ksIdealLine: from the
  same inputs b=0x3c23d70b, t=0x3ec7e000, c=0x3f6832c0 Direct3D 11 gives b·(t·c)=0x3b680d9b, the
  IEEE result of the shader's own order, and both wgpu paths give (b·t)·c=0x3b680d9a. Our WGSL keeps
  the bytecode's order (checked in the dump); the regrouping happens in the compilers below wgpu
  (FXC on the D3D12 path even with optimisation off, SwiftShader's JIT on Vulkan). WGSL has no
  `precise`, so the translation cannot pin it. The other four are taken to be the same kind; only
  ksIdealLine was taken apart.

How to run (about 20 s each):

```sh
export VK_DRIVER_FILES="$(cygpath -w $PWD/re/scratch/task38b/swiftshader/vk_swiftshader_icd.json)"
cargo run --release -p rustyac-render --features wgpu --example shader_proof -- --adapter swiftshader --flat
cargo run --release -p rustyac-render --features wgpu --example shader_proof -- --adapter warp --flat
cargo run --release -p rustyac-render --features wgpu --example shader_proof -- --adapter swiftshader --ops
```

All 72 pairs (the noise column is the textured run, filtering included):

| pair | WARP D3D11 vs SwiftShader, flat textures | WARP D3D11 vs WARP D3D12, flat textures | SwiftShader, noise textures (largest ULP) | why not the same bits |
|---|---|---|---|---|
| GL | same bits | same bits | 0 |  |
| GL2D | same bits | same bits | 0 |  |
| GLTextured | same bits | same bits | 797997 |  |
| ksBrakeDisc | 2 ULP (3989/4096 same) | lost device | 6083509 | SwiftShader's exp2/log2 |
| ksBrokenGlass | 5 ULP (3003/4096 same) | lost device | 456824 | SwiftShader's exp2/log2 |
| ksCameraDirt | same bits | same bits | 81149 |  |
| ksCarPaintSimple | 2 ULP (4035/4096 same) | lost device | 1449338 | SwiftShader's exp2/log2 |
| ksCircularRPM | same bits | same bits | 568350 |  |
| ksClouds | same bits | same bits | 318478 |  |
| ksColourShader | same bits | same bits | 0 |  |
| ksFXAA | same bits | same bits | 6035751 |  |
| ksFakeCarShadows | same bits | same bits | 92307 |  |
| ksFakeCarShadowsGen | same bits | same bits | 0 |  |
| ksFlags | 20 ULP (1867/4096 same) | lost device | 839729 | SwiftShader's exp2/log2/sin |
| ksFont | same bits | same bits | 148096 |  |
| ksGrass | same bits | lost device | 359388 |  |
| ksHighPass | same bits | same bits | 984504320 |  |
| ksIdealLine | 2 ULP (3155/4096 same) | 2 ULP (3748/4096 same) | 388909 | a chain of multiplies regrouped under wgpu (both paths) |
| ksMSDepthResolve | same bits | same bits | 0 |  |
| ksMegaShader | 4 ULP (3830/4096 same) | 3 ULP (3053/4096 same) | 80679 | a chain of multiplies regrouped under wgpu (both paths) |
| ksMultilayer | 2 ULP (3713/4096 same) | lost device | 300452 | SwiftShader's exp2/log2 |
| ksMultilayer_fresnel_nm | 2 ULP (3903/4096 same) | lost device | 694393 | SwiftShader's exp2/log2 |
| ksMultilayer_objsp | same bits | lost device | 263342 |  |
| ksOrenNayar | same bits | lost device | 147260 |  |
| ksParticle | 2 ULP (3992/4096 same) | lost device | 165163 | SwiftShader's exp2/log2 |
| ksPerPixel | same bits | lost device | 760493 |  |
| ksPerPixelAT | 2 ULP (3972/4096 same) | lost device | 179717 | SwiftShader's exp2/log2 |
| ksPerPixelAT_NM | 2 ULP (3952/4096 same) | lost device | 474500 | SwiftShader's exp2/log2 |
| ksPerPixelAT_NS | 2 ULP (3702/4096 same) | 3 ULP (3868/4096 same) | 424086 | a chain of multiplies regrouped under wgpu (both paths) |
| ksPerPixelAlpha | 4 ULP (3110/4096 same) | lost device | 231531 | SwiftShader's exp2/log2 |
| ksPerPixelMultiMap | 1 ULP (4067/4096 same) | lost device | 266177 | SwiftShader's exp2/log2 |
| ksPerPixelMultiMapSimpleRefl | same bits | lost device | 821497 |  |
| ksPerPixelMultiMap_AT | 1 ULP (4061/4096 same) | lost device | 305142 | SwiftShader's exp2/log2 |
| ksPerPixelMultiMap_AT_NMDetail | 3 ULP (3606/4096 same) | lost device | 530293 | SwiftShader's exp2/log2 |
| ksPerPixelMultiMap_NMDetail | 4 ULP (3910/4096 same) | lost device | 3999508 | SwiftShader's exp2/log2 |
| ksPerPixelMultiMap_damage | same bits | lost device | 3003145 |  |
| ksPerPixelMultiMap_damage_dirt | 16 ULP (3719/4096 same) | lost device | 455657 | SwiftShader's exp2/log2 |
| ksPerPixelNM | 3 ULP (3858/4096 same) | lost device | 11967208 | SwiftShader's exp2/log2 |
| ksPerPixelNM_UV2 | 51 ULP (3220/4096 same) | lost device | 343724 | SwiftShader's exp2/log2 |
| ksPerPixelNM_UVMult | 2 ULP (3891/4096 same) | lost device | 1021039 | SwiftShader's exp2/log2 |
| ksPerPixelReflection | 4 ULP (3759/4096 same) | lost device | 353622 | SwiftShader's exp2/log2 |
| ksPerPixelSimpleRefl | 2 ULP (3946/4096 same) | lost device | 148438 | SwiftShader's exp2/log2 |
| ksPerPixel_dual_layer | 2 ULP (3879/4096 same) | lost device | 53180 | SwiftShader's exp2/log2 |
| ksPerPixel_nosdw | 2 ULP (3997/4096 same) | 2 ULP (3999/4096 same) | 219085 | a chain of multiplies regrouped under wgpu (both paths) |
| ksPostAdaptLum | 1 ULP (0/4096 same) | same bits | 67375 | SwiftShader's exp2 (WARP D3D12: same bits) |
| ksPostBW | same bits | same bits | 75025 |  |
| ksPostBlur | same bits | same bits | 436780 |  |
| ksPostBlurH | same bits | crash | 48914 |  |
| ksPostBlurRadial | same bits | same bits | 183975 |  |
| ksPostBlurRadialMS | same bits | same bits | 0 |  |
| ksPostBlurV | same bits | crash | 24619 |  |
| ksPostBlur_MS | same bits | same bits | 0 |  |
| ksPostCopy | same bits | same bits | 121580 |  |
| ksPostCopyLuma | same bits | same bits | 32337 |  |
| ksPostFOG | same bits | crash | 25509 |  |
| ksPostFOG_MS | same bits | crash | 16541 |  |
| ksPostToneMap | same bits | same bits | 98038 |  |
| ksSelectedMesh | same bits | same bits | 0 |  |
| ksShadowGen | same bits | same bits | 0 |  |
| ksShadowGenAT | same bits | same bits | 0 |  |
| ksShadowGenSKIN | same bits | crash | 0 |  |
| ksShadowGen_debug | same bits | same bits | 0 |  |
| ksSimpleShader | same bits | same bits | 309173 |  |
| ksSkidMark | 1 ULP (4064/4096 same) | lost device | 1181466 | SwiftShader's exp2/log2 |
| ksSkinnedMesh | 8 ULP (3478/4096 same) | crash | 827373 | SwiftShader's exp2/log2 |
| ksSkinnedMesh_NMDetaill | 3 ULP (3444/4096 same) | crash | 784623 | SwiftShader's exp2/log2 |
| ksSky | same bits | same bits | 0 |  |
| ksSkyCubemap | same bits | same bits | 0 |  |
| ksTest | same bits | same bits | 0 |  |
| ksTree | 4 ULP (4022/4096 same) | 2 ULP (3980/4096 same) | 554553 | a chain of multiplies regrouped under wgpu (both paths) |
| ksTyres | 4 ULP (3447/4096 same) | lost device | 1061297 | SwiftShader's exp2/log2 |
| ksWindscreen | 2 ULP (4019/4096 same) | lost device | 324347 | SwiftShader's exp2 |

## Steps 2–4: not done (Task 38c)

Step 1 took the time: finding why WARP's Direct3D 12 driver fails needed one cut-down shader after
another, and the second rasterizer, the depth and multisample inputs and the single-instruction
mode had to exist before every pair could be compared. The brief's order puts steps 2–4 after
it and says nothing half-wired, so none of them was started; the page draws with its debug renderer
as in v0.38.0. What Task 38c starts from:

- **Step 2, kgl behind one interface.** kgl's public surface is `Kgl` (`kgl.rs`, about 45
  methods: states, samplers, render targets, cbuffers, vertex/index buffers, draws, cube maps,
  read-back, swap) plus `KglRenderTarget`, `KglCubeMap`, `KglVertexBuffer`, `KglIndexBuffer`,
  `KglCBuffer`. D3D11 types also appear outside kgl in `shader.rs`, `texture.rs`,
  `texture_fallback.rs`, `graphics.rs`, `post.rs`, `forward.rs`, `mirror.rs`, `fake_shadow.rs`,
  `fw1.rs`, `gui_paint.rs` and `gpulog.rs` (raw `ID3D11*` views, `DXGI_FORMAT`s, the shader
  objects). Plan as in Task 38: a `wgpu` feature that swaps `kgl.rs` for `kgl_wgpu.rs` with the same
  methods, neutral type names (re-exported per backend: `Format`, `TextureView`, `Sampler`,
  `ShaderSet`), the device calls of those files moved behind kgl methods; the D3D11 build must keep
  every command-log line and pixel (the goldens and the render oracle decide). The wgpu side binds
  through `rustyac_dxbc::parse` reflection and `translate_pair`, cbuffers as `array<vec4<u32>>`
  storage of the same bytes, comparison-sampled slots as depth textures, as the proof harness does.
- **Step 3, render oracle through native wgpu.** With step 2, render_oracle sequences gain a
  `--backend wgpu`: kgl call logs compared line for line with the D3D11 path, PSNR and the largest
  pixel difference per frame against the game's WARP pixels, side-by-side PNGs ≤300 KB. Expect
  SwiftShader-class differences only where the proof above found them (exp2/log2 and the regrouped
  multiplies) plus texture filtering, which the flat-texture runs took out on purpose.
- **Step 4, Full mode in the page.** rustyac-dxbc already builds for `wasm32-unknown-unknown` (clippy
  clean, this task); not yet run there. rustyac-render needs the `windows` crate behind
  `cfg(windows)` and the wgpu backend for wasm. Then the mode switch on the controls page (gear),
  remembered per browser, Full when WebGPU is there, else Debug with the reason shown.
- Leftovers of step 1: SwiftShader's exp2/log2/sin/cos are not WARP's, so for those 27 pairs the
  proof can only bound the difference; a software `exp2`/`log2` in WGSL that matches WARP's bits
  would close it (Task 38c or later, only if the frames of step 3 need it). The four other
  regrouped pairs (ksMegaShader, ksPerPixelAT_NS, ksPerPixel_nosdw, ksTree) were not taken apart
  one by one.

## Step 5: the full end-to-end checks

The renderer, physics, game, net, GUI, audio and launcher sources are byte for byte v0.38.0's
(`git diff v0.38.0 -- crates/*/src tools web` is empty; only the proof example, the renderer's
Cargo features and LICENSING.md changed). Everything was still rerun:

| Check | Result |
|---|---|
| clippy `-D warnings --all-targets`, native workspace; `rustyac-web` (all targets), `rustyac-dxbc`, `rustyac-net` for `wasm32-unknown-unknown`; math/ode/content/physics/net/game/dxbc for `wasm32-wasip1` | clean |
| `cargo build --release --locked --workspace` and every `tools/*` lock file | all build |
| `cargo test --release --locked --workspace` (all six render goldens, `dxbc_reflect`, replays, profile, ...) | 68 suites, 315 passed, 0 failed, 1 ignored |
| Chassis replays: `chassis_compare` over every recording in `oracle/car*` | 49 of 49 scenarios bit-exact at every step |
| car_oracle `all` (every scenario twice in fresh processes, the game's own code) and chassis_compare on those fresh recordings | 12 deterministic; 11 of 11 bit-exact |
| Contacts: `collide --track spa`, `collide-worlds` (7 worlds × 3,000 steps) | no difference; 100 % |
| Tracks: `rays` 300,000 each on magione, spa, ks_laguna_seca | no difference |
| Several cars from saved recordings (`multi-check`: pile-up, rear end, side by side, side swipe, AI solo) with 0, 1, 8 workers | all IDENTICAL, thread counts the same |
| Several cars recorded afresh (`multi`: mc_rear_end, mc_pile_up, mc_mixed at Spa; ai_race, pit_box, rc_cut_practice at Magione) with 0, 1, 8 workers | all IDENTICAL, every race_out.json identical |
| rc_jump (jump start) recorded afresh | **differs** at step 2717 (`car2.ai.desiredOffset`); Task 28's saved rc_jump recordings (`oracle/t28/f_jump`, `oracle/t28/jump`) replay IDENTICAL with 0, 1, 8 workers and identical race_out.json: see Open questions |
| AC's replay recorder (`multi --replay`: mc_pile_up, ai_race, mc_mixed: playback, a cut file, playback of the cut file), ghost car and comparative telemetry (`--ghost`, `--telemetry`) | all IDENTICAL |
| rustyac.exe: a 60 s drive recorded, replayed with 0, 1, 8 workers | one state hash `6ebb7135f37c1373` |
| Render oracle: Spa chase / cockpit / far, Magione chase; F2004 spa_launch 39 frames; E30 spa_wall_slide 39 frames with post-processing and FXAA | command log and pixels IDENTICAL in every frame |
| GUI sequences of Tasks 29–34 (60: HUD, widgets, forms, leaderboards, end-of-session, ESC menus, replay bar, pit menu, setup editor, apps) | 60 of 60 identical (57 by pixels and command log, 3 by the files they write) |
| Sound: the 18 drives of `re/scratch/task19/compare_all.sh` through audio_oracle | 18 of 18 `ok`, sound calls identical in every drive (the mix columns vary from run to run as in Task 19: FMOD) |
| Multiplayer goldens (`tools/net_gold/check.sh`): 20 tapes × desktop, WebSocket, gateway, wasm | 80 of 80 IDENTICAL at every step |
| Launcher answers: 30 requests to the launcher oracle (AC's own bridge) and to the port | 27 identical; the 3 others are the port's fixed test host (screen modes, primary resolution, the clock in `getresults`) |
| Browser drives: all 156 recorded drives in `oracle/game`, desktop vs `wasm32-wasip1` under wasmtime | 156 of 156 identical (1,618,668 steps) |
| AI in the page: Task 37c's Quick Race (seed 28005, the desktop's tape): desktop replay, native `race_proof`, the page in headless Chrome (site built afresh) | chain `7aea29db0a574b43` all three, 93,774 steps, race_out.json 1,264 B identical; the page at 60 fps, no errors |
| Physics dumps: `--dump-states` of every fourth drive, v0.38.0's rustyac.exe vs this one | 39 of 39 byte-identical |

## New or renamed files in the web zip (38b)

None. `web/` and `crates/rustyac-web` did not change.

## Disk (38b)

35 GB free at the start, 34 GB at the end. Deleted at the end: the old-version build
(`re/scratch/task38b/tgt_old`) and its worktree.

## Open questions (38b, decided without asking)

- The second software reference is SwiftShader (Vulkan) copied from the installed Chrome into
  ignored scratch, not lavapipe: it was already on the PC, is what Chrome itself falls back to, and
  needs no download. It is Google's file and stays out of the repository.
- Pairs are compared with flat textures for the bit-for-bit verdict; the noise-texture run is kept
  as a column, since its differences are the two rasterizers' texture filtering, not the shaders.
- The WARP D3D12 failures are not worked around in wgpu (patching naga's comparison-sampler heap or
  its loop code would test a different translation from the one the browser gets).
- The launcher probe uses a short hand-written list (30 requests) because Task 35–36's request
  lists went with their scratch folder; the three different answers come from the port's fixed
  test host (screen modes, primary resolution, the clock in `getresults`), as before.
- Steps 2–4 were left whole for Task 38c rather than started, as the brief asks.
- `rc_jump` recorded afresh does not agree with the port (step 2717, an AI car's desired offset;
  the fresh game run also ends differently from Task 28's: race_out.json 2,339 B against 2,106 B,
  2,715 locked frames against 2,682). The port's code is the same as v0.37.2's and Task 28's two
  saved rc_jump recordings still replay IDENTICAL, so this is not from 38b: the game side of a
  fresh run sees something different from Task 28's (most likely a file in the scratch game
  folder or the user's ini files). Not chased here; Task 38c should record it again and diff the
  game's inputs.
- "Tracks" was rerun as rays on three tracks, contacts and the per-track drives in `oracle/game`;
  Task 18's every-track survey scripts are gone with their scratch folder.
