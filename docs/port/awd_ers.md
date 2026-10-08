# Task 16: grip investigation, four-wheel drive, KERS and ERS

## Resume here

State after the last commit. **The task stopped early (usage limit): no version bump, no tag.**

Done and committed:

- **Step 0 (grip):** `docs/port/grip_investigation.md` (ranked suspects, ruled-out list, CSP list, the eight
  telemetry runs). No fault found in rustyAC physics; no physics fix.
- **Step 1 (port):** 4WD (`step_4wd`, `step_4wd_new`, controllers, ten setup items), KERS (`car/kers.rs`), ERS
  (`car/ers.rs`: MGU-K, MGU-H, front motors, cockpit settings), shared memory, HUD lines, controls
  (pad B / key K = KERS; Insert / Delete engine brake, Home / End recovery, PageUp / PageDown delivery, M MGU-H;
  Alt or Right Ctrl + 1..4, Shift = down). Side find: the track DRS zones (`data/drs_zones.ini`).
- **Step 2 (oracle):** 83 scenarios, 569,095 steps, **all 100 % bit-exact** against the final code
  (`re/scratch/task16/final_compare.txt`, made by `final_compare.sh`): 8 installed 4WD cars and 3 made-up ones
  on the flat road, Sesto and R8 on Spa, 6 hybrids on the flat road, F138 and SF15-T on Spa (laps counted, lap
  reset, DRS opening only in zones: `spa_drs_timing`), 7 made-up hybrids and 2 more mod cars
  (`re/scratch/task16/record_testcars.sh`). `rustyac.exe` itself replays 4 of them bit-exact
  (`chassis_compare game-replay`). Faults (`chassis_compare faults16`): all noticed. Golden tests: one AWD and
  one ERS excerpt in `crates/rustyac-physics/tests/chassis_golden.rs`.
  Refused cars: 57 of 123 before (`re/scratch/task16/acd_before.txt`), 40 after (`acd_final.txt`: 38 not
  double-wishbone, 2 rear-wheel steering).
- **Step 3 (leftovers):** assists.ini shift aids, hot-lap arming by session type, two screenshots
  (`awd_sesto_spa.png`, `ers_sf15t_spa.png`).
- **Review** (`re/scratch/task16/review_task16.md`): D1, D2, R1, R2, R3, N2 fixed in `4f12b53`.

Still to do:

1. **This report**: only this block exists. Write the plain-English summary, what is ported with AC addresses,
   the result tables, the refused list, the Formula Alpha 2026 section (from
   `re/scratch/task16/alpha2026_csp.md`), new controls, performance (headless Spa, average step: F2004 0.089 ms,
   Sesto 0.079, R8 0.082, F138 0.092, SF15-T 0.089), open questions (review notes N3 to N8; BRAKE_ENGINE of a
   saved setup not applied; `hy_modes` not game-replayed; `vrc_formula_lithium_2023_csp` loads but was never
   compared; `--autodrive` cannot lap Spa in a car without traction control).
2. The last made-up car change (`lithium_ers_front` with live front maps) is recorded and 100 %, but
   `coverage16.py` and `faults16 hy_deploy --dir oracle/hyt_lithium_ers_front` were not run again on it.
3. Bring `docs/port/conditions.md` (rows on the shift aids and the session type) in line; update the memory
   notes.
4. Clean up: delete `oracle/awd*`, `oracle/hy*` (large); remove the junction
   `re\scratch\task16\v0150\cardata` with `cmd /c rmdir` (never a recursive delete), then
   `git worktree remove re/scratch/task16/v0150`.
5. **Versioning last** (`docs/release.md`): `0.16.0`, lock files, `chore(release): v0.16.0`, tag `v0.16.0`.
