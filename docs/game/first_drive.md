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
- **Next:** physics thread at 333 Hz with timing statistics (`--headless --duration`), shared memory, input
  devices and bindings, the Direct3D 11 debug view and `--screenshot`, then this report.
- Specs written by the read-only helpers are in git-ignored `re/scratch/task11/spec_*.md`.

(The report proper follows once the pieces exist.)
