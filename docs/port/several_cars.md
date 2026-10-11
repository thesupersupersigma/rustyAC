# Task 24: several cars on track

## Resume here

State after the last commit (update after every commit):

- **Done:** the several-car engine (`crates/rustyac-physics/src/engine/`: one world, a working copy
  per car, the thread pool), car-vs-car contacts and damage, the pit-lane rule, the several-car
  oracle (`car_oracle multi`, `multi-check`) with the first five scenarios bit-exact on 0 and 8
  worker threads.
- **Running / next:** the six-car lap (`mc_field`) and the mixed cars (`mc_mixed`); then the
  drivers of the other cars in `rustyac.exe` (step 3), the golden test (step 6), performance
  (step 7), the report's tables.
- **Not started:** drawing and hearing the other cars (step 5).
- Specs read from the machine code: `re/scratch/task24/spec_*.md` (git-ignored).
- How to re-run a proof: `tools/car_oracle/target/release/car_oracle.exe multi --scenario
  mc_rear_end --track spa` (records the game, then replays the port with 0 and N threads).

(The full report is written at the end of the task.)
