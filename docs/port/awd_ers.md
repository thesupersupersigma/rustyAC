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
- **Step 2 (oracle):** `car_oracle` records the 4WD members for a 4WD car. First recordings, each
  `pt_autoshift`, `wc_stops`, `wc_spirited` on the flat road: `ks_lamborghini_sesto_elemento` (AWD) and
  `ks_audi_r8_plus` (AWD2), both **100 % bit-exact (12,336 steps each)**.
  `car_oracle all --car <car> --out oracle/<dir> --only pt_autoshift,wc_stops,wc_spirited`, then
  `chassis_compare run --dir oracle/<dir>`. Still to do: the other AWD2 cars, Spa, made-up cars for the two
  AWD controller files and an AWD2 car without `ctrl_awd2.ini`, faults, golden excerpts.
- **Step 3 (leftovers):** not started. Also to do there (promised in the grip report): D1 session start time,
  D2 `AUTO_BLIP` from assists.ini, D3 first lap armed only in a hot-lap session.
- **Versioning:** not done. No tag yet.

Next: KERS and ERS from the briefs; then the remaining 4WD recordings (Spa, the other cars, test cars).
