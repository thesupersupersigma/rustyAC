# Task 24b: every car drawn and heard

## Resume here

**State (work in progress, not released yet):** items 1 to 6 of the task are written and their
new proofs pass. Still to do before the release: the older proof batches again (running), the
one-car check against v0.24.0, the performance numbers, the screenshots, this report in full,
the version and the tag.

- Done and proven against the game:
  - every car drawn (`CarAvatar` per car, `CarNodeSorter`, the smoothed state of the other
    cars, the smoke cap, the mirror from the focused car): 19 `render_oracle` sequences,
    command log and WARP pixels identical (`re/scratch/task24b/batch.sh`);
  - the focused car, its keys and the camera's fade (2 of the 19 sequences);
  - every car heard: 14 `audio_oracle` drives, call logs identical
    (`re/scratch/task24b/audio_batch.sh`);
  - race.ini's `[CAR_n]` cars: 9 readings identical with the game's own
    `RaceManager::initOffline` (`car_oracle race-ini`);
  - `--graphics max` leaves post-processing off;
  - the golden frame of three cars (`cargo test --release -p rustyac-render --test golden_cars`).
- Tapes of several cars: `car_oracle multi --scenario <mc_...> --track <t> --audio-tape --out <dir>`
  writes `<scenario>.car<n>.audiotape`; the ones of the proofs are in `oracle/t24b/` (git-ignored).
- Briefs read from the machine code: `re/scratch/task24b/spec_a_render_ncars.md`,
  `spec_b_audio_ncars.md`, `spec_c_initoffline_harness.md` (git-ignored).

(The full report follows when the last checks are in.)
