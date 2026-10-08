# Task 17: every suspension, rear-wheel steering and the old tyre model

## Resume here

Task brief: `prompts/17_suspensions_rearsteer_oldtyre.md`. Work happens on `master`, one commit
per working piece, no tag until everything is done.

State after the last commit:

| Piece | State |
|---|---|
| `SuspensionStrut` (`car/suspension_strut.rs`) | ported; `bmw_m3_e30` bit-exact on `wc_stops` (600 steps) and `wc_spirited` (4001 steps) |
| `SuspensionAxle` (`car/suspension_axle.rs`) + the drivetrain's axle torque reaction | ported; `ks_ferrari_250_gto` bit-exact on `wc_stops` and `wc_spirited` |
| `tools/car_oracle`, `tools/chassis_compare` | record and compare any of the four suspension classes (`replay::CarLayout`) |
| `SuspensionML` | not started |
| Rear-wheel steering | not started |
| Old tyre model (`VERSION < 10`) | not started |
| Micro-oracles, the full scenario sets, golden tests, rustyac.exe replays, report | not started |

Next steps, in order: ML, rear steer, old tyre (briefs `spec_ml.md`, `spec_rearsteer.md`,
`spec_oldtyre.md`), then step 2 of the task brief.

Recordings so far (git-ignored, re-record with `car_oracle run --car <car> --scenario <s> --out <dir>`):
`oracle/t17_e30`, `oracle/t17_gto`.

Briefs read from the machine code (git-ignored): `re/scratch/task17/spec_*.md`.
