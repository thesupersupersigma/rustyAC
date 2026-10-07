# The whole Rust car: aero, aids, `Car::step` shell, telemetry page (Task 10)

## Resume here

(Kept up to date while the task runs, so that a cut-off loses nothing. Newest state first.)

- **State**: aero is ported and committed (`car/aero.rs`: `AeroModel` slot, `VanillaAero` with
  wings, fins, ride-height tables, damage factor, wing controllers, DRS, air density, slipstream
  hook, ground wind; `WING_n` setup items; `RollingChassis::install_aero`). With the wings in
  Rust every existing recording is still bit-exact (11 F2004 drives and the 8 folders of extra
  cars, 187,880 steps, force tape and per-wing values included). `chassis_compare run --feed
  aero` puts the recorded wing forces back. Golden files are now format `CHGOLD03`.
  Next: the aids. Four read-only helpers are writing instruction-level briefs from the
  disassembly into the git-ignored `re/scratch/task10/` (`spec_aero.md`, `spec_aids.md`,
  `spec_car_step.md`, `spec_telemetry.md`); the aero port was written from the listings there
  (`ae_*.asm`) and still has to be read against `spec_aero.md` (loader details, the damage
  coefficients, `SlipStream::getSlipEffect`).
- **Plan** (order of the task, one commit each): aero (`AeroModel` slot + `VanillaAero` in
  `crates/rustyac-physics/src/car/aero.rs`) → aids (`AidsModel` slot, `aids.rs`) → the
  `Car::step` shell (`VanillaCar`) → the telemetry page (`telemetry.rs`) → `chassis_compare`
  with a mode fed only the driver's raw controls and the session / track values → extra
  oracle scenarios (DRS, traction control, ABS, pit limiter, active aero) → faults, golden
  test, this report.
- **What already exists that this task builds on**: the recordings of `tools/car_oracle`
  already hold, per step, the air density, each wing's angle of attack / CD / CL / angle /
  ground height / drag / lift / yaw angle, the aids' switches and outputs (`tc.*`, `abs.*`,
  `edl.*`, `stability.*`, `speedLimiter.*`, `drs.*`) and the whole `acpmf_physics` page
  (`page.*`, 148 values), so the old recordings can check the new systems without being made
  again.
- **To check that nothing broke** (no game needed): `cargo test -p rustyac-physics`, then
  `chassis_compare run` (see `docs/port/drivetrain.md` section 5).
