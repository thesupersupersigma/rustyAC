# Task 17: every suspension, rear-wheel steering and the old tyre model

## Resume here

Task brief: `prompts/17_suspensions_rearsteer_oldtyre.md`. Work happens on `master`, one commit
per working piece, no tag until everything is done.

State after the last commit:

| Piece | State |
|---|---|
| `SuspensionStrut` port (`crates/rustyac-physics/src/car/suspension_strut.rs`) | written, builds, the 29 strut cars load and run 300 steps; **not yet compared with the game** |
| Recorded field lists for any car layout (`replay::CarLayout`) | done on the Rust side; `tools/car_oracle` still records double-wishbone cars only |
| `SuspensionAxle`, `SuspensionML` | not started |
| Rear-wheel steering | not started |
| Old tyre model (`VERSION < 10`) | not started |
| Oracles, recordings, golden tests, report | not started |

Next steps, in order:
1. `tools/car_oracle/src/game.rs`: `find_bodies_and_joints` must accept the other three
   suspension classes (per-class offsets of hub / joints / status / steer torque, the strut
   body, the rigid axle, ball and slider joint parameters).
2. Record `bmw_m3_e30` and hold the strut port against it (`chassis_compare run`).
3. Axle, ML, rear steer, old tyre, in the order of the brief.

Briefs read from the machine code (git-ignored): `re/scratch/task17/spec_*.md`.
