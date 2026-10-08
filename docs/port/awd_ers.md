# Task 16: grip investigation, four-wheel drive, KERS and ERS

## Resume here

State after the last commit (update this block with every commit):

- **Step 0 (grip): done.** Report: `docs/port/grip_investigation.md`. Briefs: `re/scratch/task16/csp_survey.md`,
  `oracle_vs_session.md`. Tools: `tools/corner_numbers.py`, the `grip_probe` example,
  `rustyac.exe --pressure-law`.
- **Step 1 (port):**
  - 4WD: **done and committed** (`drivetrain.rs`: `step_4wd`, `step_4wd_new`, the three controllers, the loader;
    `setup.rs`: the ten 4WD items), from `re/scratch/task16/spec_awd.md`.
  - KERS, ERS, shared memory, HUD, controls: not started. Briefs: `spec_kers.md`, `spec_ers.md`,
    `spec_shm_controls.md`, `spec_leftovers.md` in `re/scratch/task16/` (written by the read-only workflow).
- **Step 2 (oracle):** 4WD is done: all eight loadable 4WD cars on the flat road (`pt_autoshift`, `wc_stops`,
  `wc_spirited`: 12,336 steps each), the Sesto Elemento and the R8 Plus on Spa (`spa_launch`, `spa_kerbs`,
  `spa_lap`), and three made-up cars (`chassis_compare test-car`: `sesto_awd_ctrl`, `r8_awd2_plain`,
  `r8_awd2_spool`): **every step bit-exact**. Recordings in `oracle/awd*` (git-ignored), result files
  `oracle/chassis/results_awd*_whole.md`, logs `re/scratch/task16/cmp_*.txt`.
  How: `car_oracle all --car <car> --out oracle/<dir> --only pt_autoshift,wc_stops,wc_spirited`;
  `car_oracle run --track spa --scenario spa_launch --car <car> --out oracle/<dir>`;
  `chassis_compare run --dir oracle/<dir>`. Still to do for 4WD: faults, a golden excerpt.
  The refused-car list before this task: `re/scratch/task16/acd_before.txt` (made with v0.15.0 checked out
  into `re/scratch/task16/v0150`, a git worktree: remove it at the end with `git worktree remove`).
  `acd_check.exe --refused` now builds every installed car, also those without an extracted folder.
- **Step 3 (leftovers): done** except the screenshots: assists.ini `AUTO_CLUTCH` / `AUTO_BLIP` /
  `AUTO_SHIFTER` applied as the game does (flags `--auto-clutch`, `--no-auto-clutch`, `--auto-shifter`,
  `--no-auto-shifter`, `--auto-blip`, `--no-auto-blip` on top), the session type and spawn set from race.ini
  (the first lap is armed only in a hot-lap session), the session's start time (D1 of the grip report).
  `spec_leftovers.md` (when the workflow has written it) should be read against this.
- **Versioning:** not done. No tag yet.

Next: KERS and ERS from the briefs; then the remaining 4WD recordings (Spa, the other cars, test cars).
