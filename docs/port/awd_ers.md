# Task 16: grip investigation, four-wheel drive, KERS and ERS

## Resume here

State after the last commit (update this block with every commit):

- **Step 0 (grip): done.** Report: `docs/port/grip_investigation.md`. Briefs: `re/scratch/task16/csp_survey.md`,
  `oracle_vs_session.md`. Tools: `tools/corner_numbers.py`, the `grip_probe` example,
  `rustyac.exe --pressure-law`.
- **Step 1 (port):**
  - 4WD (`step4WD`, `step4WD_new`, controllers, loader, the ten setup items): written in
    `crates/rustyac-physics/src/car/drivetrain.rs` and `setup.rs` from `re/scratch/task16/spec_awd.md`;
    **not committed yet, not yet held against the game** (old cars' tests pass).
  - KERS, ERS, shared memory, HUD, controls: not started. Briefs still being written by the read-only
    workflow (`spec_kers.md`, `spec_ers.md`, `spec_shm_controls.md`, `spec_leftovers.md` in
    `re/scratch/task16/`).
- **Step 2 (oracle):** not started. Test cars: AWD `ks_lamborghini_sesto_elemento` (the only AWD car with
  double wishbones all round), AWD2 `ks_audi_r8_plus`, `ks_nissan_gtr`, `ks_lamborghini_huracan_performante`
  and four more; made-up cars for the two AWD controller files.
- **Step 3 (leftovers):** not started. Also to do there (promised in the grip report): D1 session start time,
  D2 `AUTO_BLIP` from assists.ini, D3 first lap armed only in a hot-lap session.
- **Versioning:** not done. No tag yet.

Next: build `car_oracle` with the 4WD fields recorded, record the Sesto Elemento and an AWD2 car, compare.
