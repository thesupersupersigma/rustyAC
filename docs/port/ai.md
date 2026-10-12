# AI drivers (Task 27)

## Resume here

State: **in progress**. Last commit: the first one of this task (the line, the driver, the
engine's serial AI step, the oracle; one F2004 lap of Spa is bit-exact).

- Done so far: `crates/rustyac-physics/src/ai/` (`driver.rs` = `AIDriver`, `awareness.rs` =
  `stepCarsAwareness2`, `engineer.rs` = the `RaceEngineer` parts, `car_ext.rs` = the `Car`
  parts, `oracle.rs` = the column table of the comparison), `track/ai_line.rs` (the rest of
  `AISpline`, `AISplineRecorder`, version 6 lines), `msvc_sort.rs`, the engine's AI step
  (`engine/mod.rs`: `step_cars`, `set_session`, `deliver_forced_positions`), the recording
  (`engine/record.rs`: `AiMeta`, AI columns), `car_oracle multi --scenario ai_*`.
- Proven: `ai_solo` on Spa (F2004, 42,000 steps): IDENTICAL.
- Next: AI against AI (`ai_race`, `ai_field` with 2 / 6 / 12 cars), other cars and tracks,
  levels, a version 6 line, the pit lane, `AIDriverDrag`, the hand-over, `rustyac.exe`
  wiring, the golden test, screenshots, performance, this report, the version.
- How to check: `tools/car_oracle/target/release/car_oracle.exe multi --scenario ai_solo
  --track spa --steps 42000 --threads 0,1,8 --out oracle/t27` (records the game, then replays
  the port), or `sh re/scratch/task27/t.sh oracle/t27/ai_solo.mcrec` (rebuild + replay only).
- Briefs read from the machine code: `re/scratch/task27/spec_spline.md`,
  `spec_harness.md`, `spec_engineer_awareness.md` (git-ignored).
