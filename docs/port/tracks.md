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
- Next, in this order: Task 12's loader gaps (`ROTATION`, missing `MODEL_n`, old AI lines, the lookup grid,
  the unbounded ray, sub-space 0, late meshes), point-to-point timing, loose track objects, the rays on every
  track, the car scenarios on the test set, golden excerpt, screenshots, performance, the doc refresh of
  `docs/map/suspension.md` and `docs/map/tyre.md`, then the version and the tag.
- Briefs read from the machine code (git-ignored): `re/scratch/task18/spec_*.md`. Patch scripts and helper
  scripts are in the same folder.

(The report itself is written when the work is done.)
