# Task 12: the first real track (Spa)

## Resume here

State after the last commit (update this section after every commit):

- **Done and committed:** `crates/rustyac-content` (kn5 reader), `rustyac-ode` stage 3 part 1
  (`opcode.rs`: tree build, `collision.rs`: static meshes in simple spaces + the ray),
  `rustyac-physics/src/track/` (`surfaces.rs`, `mod.rs` with `Track` / `TrackGround` /
  `DynamicTrack`, `loader.rs` with the physics meshes and helper nodes), the tool
  `track_info`, `tools/car_oracle/src/track.rs` with `car_oracle rays` (the ray micro-oracle:
  1,000,000 / 1,000,000 rays bit-identical on Spa, results in git-ignored
  `oracle/track/rays_results.md`).
- **Placeholders, not ported yet:** `track/spline.rs` (AI line, `SplineLocator`) and
  `track/timing.rs` (timing lines, `TimeTransponder`, `LapInvalidator`, spawn sets): the
  functions exist and do nothing.
- **In the working tree, compiled but not wired in:** `crates/rustyac-game/src/render/dds.rs`,
  `render/models.rs` and the model drawing in `render/mod.rs` (track and car kn5 with
  diffuse textures). `main.rs` / `cli.rs` do not know `--track` yet.
- **Next:** (1) spline + timing + spawn from the specs in git-ignored
  `re/scratch/task12/spec_spline_locator.md` and `spec_timing_spawn.md`; (2) `car_oracle run
  --track spa --scenario spa_*` (the game's car on the game's track, body as a ghost) and
  `chassis_compare` against those recordings; (3) golden test; (4) the game: `--track`,
  `--spawn`, HUD, shared memory, screenshots; (5) this report.
- Instruction-level specs written by read-only subagents: `re/scratch/task12/spec_*.md`
  (kn5 + track load, OPCODE build, ray cast; each with a review log).
- Branch: `feat/track-spa` (made from `chore/license-files`). Nothing is pushed.

How to re-run what exists:

```
cargo test --release -p rustyac-ode -p rustyac-physics -p rustyac-content
target/release/track_info.exe "C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\content\tracks\spa"
cd tools/car_oracle && cargo build --release
tools/car_oracle/target/release/car_oracle.exe rays --track spa --count 1000000
```
