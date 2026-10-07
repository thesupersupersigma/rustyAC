# First drive (Task 11)

## Resume here

State after the last commit (update this section with every commit):

- **Done:** crate `crates/rustyac-game` (`rustyac.exe`) with the simulation core (`sim.rs`: one `VanillaCar`,
  spawned like the oracle's, stepped only through `GameSim::step`), the input file of `--record` / `--replay`
  (`input_file.rs`), the state dump of `--dump-states` (`dump.rs`), the command line (`cli.rs`), and the headless
  replay (`rustyac --replay <file> --headless [--dump-states <file|->]`).
- **Check 1 passes:** `cargo test --release -p rustyac-game` (replay by the exe = `VanillaCar` stepped directly,
  5,200 steps, 14.5 million values) and `tools/chassis_compare game-replay` (the exe against every recording of
  `oracle/car`: 70,006 steps bit-exact, 2,009 + 272 values per step and 4.5 million force calls).
- **Done too:** the physics thread (`physics_thread.rs`, `timer.rs`: 333 Hz on an absolute schedule, timing
  statistics, the last two steps for the display in `view.rs`), the spawn sequence (rest, then first gear with the
  up-shift paddle; `sim.rs`), the shared memory (`shm.rs`: the three pages, refused while `acs.exe` runs), and the
  unattended modes `--headless [--duration s]` and `--headless --replay <file> --realtime`.
- **Checks 2 and 3 work** (numbers still to be taken for the report): `rustyac --headless --duration 60`, and
  `rustyac --headless --realtime --replay oracle/game/car_slalom.ryin` read by
  `python ac_telemetry.py --duration 5 --print --out re/scratch/task11/shm_test.csv`.
- **Next:** input devices and bindings (`re/scratch/task11/spec_pad_keyboard.md`, `spec_wheel_ffb.md`), the
  Direct3D 11 debug view and `--screenshot` (`spec_shm_loop_camera.md`), then this report.
- Specs written by the read-only helpers are in git-ignored `re/scratch/task11/spec_*.md`.

(The report proper follows once the pieces exist.)
