# Task 18: every track and layout

## Resume here

State after the last commit (kept up to date with every commit):

- **In progress.** Not tagged. Work is on `master`, nothing is pushed.
- Done so far:
  1. `--track <name> --layout <layout>` in `rustyac.exe`, with the track found by folder name, menu name or a
     part of either, race.ini's `CONFIG_TRACK` taken when its `TRACK` is the same track, and `--list-tracks`.
     Unknown, not-installed and CSP-only tracks are refused with the reason
     (`crates/rustyac-physics/src/track/catalog.rs`).
  2. `track_survey.exe` (the survey table) and `--layout` in `car_oracle` / `chassis_compare` (header key
     `track_layout`, input-file keys `layout` and `oracle_track_layout`).
  3. Task 12's gaps: `ROTATION` (physics and picture), a missing `MODEL_n` file, AI lines older than
     version 7, the lookup grid built with the game's own sort (unit test against the game's stored grids),
     the ray without an end (`f32::MAX`), meshes in sub-space 0, meshes added after the first ray. The last
     three are checked by `car_oracle rays [--space0 n] [--late percent]`: bit-exact on Magione.
  4. Loose track objects (`AC_POBJECT...`): in the loader, in the car's world before the car, in
     `car_oracle` (the game's own `PhysicsObject` constructor) and in the comparison. First recording
     (Laguna Seca, 33 objects asleep) is bit-exact; a scenario that hits one is next.
- Next, in this order: the scenarios of this task in `car_oracle` (a whole lap, a point-to-point run, a hit
  on a loose object), the rays on every track, the car scenarios on the test set with two cars, golden
  excerpt, screenshots, performance, the doc refresh of `docs/map/suspension.md` and `docs/map/tyre.md`, then
  the version and the tag.
- Briefs read from the machine code (git-ignored): `re/scratch/task18/spec_*.md`. Patch scripts and helper
  scripts are in the same folder.

(The report itself is written when the work is done.)
