# Race rules: sessions, start, timing, flags, penalties, results (Task 28)

## Resume here

State after the last commit (work in progress, the report below is not written yet):

- **Ported** (`crates/rustyac-physics/src/race/`): `manager.rs` (RaceManager), `timing.rs`
  (RaceTimingServices), `results.rs` (SessionResultProcessor), `flags.rs` (FlagManager's decision),
  `hud.rs` (the banner and message texts), `output.rs` (race_out.json, laps.ini is in `timing.rs`),
  `referee.rs` (the glue to the cars: one "frame" of the game's main thread every third physics
  step). `PenaltyManager::step` is in `car/chassis.rs`.
- **In the game**: `crates/rustyac-game/src/referee.rs`, `GameSim::referee`; race.ini's sessions by
  default, `--sessions p1,q1,r3`, `--no-referee`, `--next-session-at`, `--out-dir`, keys F7 / F8.
- **Oracle**: `tools/car_oracle/src/game/race.rs`.
  - `car_oracle race-micro --seed n --kind 0..7 --count <cars> --steps <frames>`: the game's own
    referee on made-up cars against the port, every field after every frame, and race_out.json.
    Batch: `sh re/scratch/task28/micro.sh [seeds per list]`.
  - `car_oracle multi --scenario rc_race|rc_timed|rc_weekend|rc_jump|rc_jump_dt|rc_cut|... --track
    magione --cars ... --threads 0,1,8 --out oracle/t28/<label>`: whole sessions, the game's
    referee among the game's own cars and AI drivers; the port replays it and must give the same
    physics rows, the same referee state every frame and the same race_out.json.
- **Next**: run and fix the `rc_*` scenarios; flags micro-oracle; online mirroring; browser HUD;
  golden tests; old-against-new replay check; the report; the deploy notes; v0.28.0.
