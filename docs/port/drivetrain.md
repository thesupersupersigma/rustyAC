# Brakes, engine and 2WD drivetrain, with `DynamicController` (Task 09)

## Resume here

(Kept up to date while the task runs, so that a cut-off loses nothing. Newest state first.)

- **State**: the port is committed and bit-exact on the F2004. `chassis_compare run` (all
  eleven contact-free recordings, 70,006 steps, brakes + engine + clutch + gearbox +
  differential + shift helpers computed in Rust, free run from step 0): 100 % bit-exact, no
  divergence; 2,009 chassis values + 51 powertrain values per step + the force tape.
  `cargo test -p rustyac-physics` passes (four golden excerpts, two of them with the
  powertrain in Rust).
- **Commits so far**: `2dc68d2` (the port, crates), `f152324` (`tools/chassis_compare`),
  `1501e78` (`tools/car_oracle` powertrain scenarios; built, **not run yet**).
- **Next steps, in order**:
  1. Branch coverage: write altered F2004 copies with `chassis_compare test-car` (turbo +
     wastegate + controllers, handbrake, disc temperatures, EBB, spool / open differential,
     H-shifter, FWD, loader fall-backs), record them with
     `car_oracle all --car <name> --out oracle/car_<name> --only <scenarios>` (the new
     `pt_*` scenarios need `--only`), compare with `chassis_compare run --dir oracle/car_<name>`.
     Keep the recordings short (rule 8).
  2. `chassis_compare faults <scenario>` with the powertrain in Rust (12 new deliberate faults).
  3. Read-only review of the branches no recording reaches, against the disassembly.
  4. The report below (sections 1 to 6), then the final commit.
- **Specs** the port was checked against (git-ignored, written by read-only helpers from the
  disassembly): `re/scratch/task09/spec_controller_brakes.md`, `spec_engine.md`,
  `spec_drivetrain.md`, `spec_assists_setup.md` (the last may still be missing; the shift
  helpers were ported from the listings in `re/scratch/task09/as/`).

---

(The report proper is written at the end of the task.)
