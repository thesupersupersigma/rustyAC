# Task 20: AC's real renderer, part 1 (shaders, materials, lighting, sky, shadows)

## Resume here

State after the last commit (work in progress, not released):

- **Done and proven on WARP against `acs.exe` (command log identical, pixels identical):** track-only
  frames of Spa, Monza, Magione and Laguna Seca with a chase camera: kgl and its state objects, the
  game's own `.fxo` shaders, kn5 materials and textures (through the player's `d3dx11_43.dll`), the
  scene graph and its culling, lighting from the ini files (sun angle, track lighting, weather
  `3_clear`), the sky dome, the three shadow cascades, the opaque and transparent passes, the
  one-time render of the reflection cube map.
- **Next:** the car (LOD A, `SuspensionAvatar` cars first, then `SuspensionAnimator` cars with
  `.ksanim`), the steering wheel, LOD switching; then `rustyac.exe` (new renderer as default,
  `--debug-view`, `--warp`, `--gpu-log`, the real `video.ini`), the 16-frame batch, the golden test,
  the GPU screenshots, performance, this report, the release.
- Specs read from the disassembly: git-ignored `re/scratch/task20/spec_1_kgl.md` …
  `spec_9_cubemap.md`.
- Run one comparison:
  `tools/render_oracle/target/release/render_oracle.exe compare --track spa --view chase --out re/scratch/task20/out`
  (build first: `cargo build --release --manifest-path tools/render_oracle/Cargo.toml`).

The report itself is written at the end of the task.
