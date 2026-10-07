# The whole Rust car: aero, aids, `Car::step` shell, telemetry page (Task 10)

## Resume here

(Kept up to date while the task runs, so that a cut-off loses nothing. Newest state first.)

- **State**: the telemetry page is ported and committed (`car/telemetry.rs`:
  `PhysicsPageWriter`, `PhysicsPage`, the 148 values of `SPageFilePhysics` after every step,
  with `Car::computeRideHeight`, the standing-car estimate `RaceEngineer::evalFront/RearRideHeight`,
  `Car::getCGHeight`, heading / pitch / roll; `atan2f` added to `rustyac-math`;
  `RollingChassis::install_telemetry`). All four systems of the task are in: with everything in
  Rust every recording is bit-exact, the page field by field (F2004 x 11 + test cars + F40 +
  488 GT3 + the new `wc_*` drives incl. Lotus Exos and Giulia QV). Left to do, in this order:
  `VanillaCar` (car + device in one type), `chassis_compare` tables for the new systems and
  its "driver only" wording, faults, golden excerpts (one F2004, one 488 GT3, driver inputs
  only), review of the untested branches against `re/scratch/task10/spec_*.md`, the report.
- **Before that**: the `Car::step` shell is ported and committed (in `car/chassis.rs`: the whole of
  `Car::pollControls` with the device's rumble and rev marks, headlight switch, black-flag
  teleport, penalty timers, `updateColliderStatus`, `stepJumpStart`, `postStep` with the wake;
  `PhysicsEngine::stepWind`; `SlipStream::setPosition`; new provider hooks on `ChassisFeed`).
  `tools/car_oracle` has six new scenarios (`wc_drs`, `wc_stops`, `wc_pit`, `wc_spirited`,
  `wc_stability`, `wc_wind`; `scenario::whole()`), recorded for the F2004 (`oracle/car_wc`), the
  488 GT3 (`oracle/car_wc_488`), the Lotus Exos 125 with DRS (`oracle/car_wc_exos`) and the
  Giulia QV with a differential lock (`oracle/car_wc_giulia`): all bit-exact in a free run, with
  the shell's values compared too. Next: the telemetry page (`car/telemetry.rs`), then a
  `VanillaCar` type that owns car + device, the compare tool's "driver only" mode and tables,
  faults, golden test, review, report.
- **Before that**: the aids are ported and committed too (`car/aids.rs`: `AidsModel` slot,
  `VanillaAids` = traction control, ABS, electronic differential lock, stability control, pit
  limiter, with `Tyre::getDX` and `RaceEngineer::getOptimalBrake`; `RollingChassis::install_aids`;
  `SurfaceDef` has `is_pitlane` / `is_valid_track`). With wings and aids in Rust every existing
  recording is bit-exact (the F2004's traction control cuts and the 488 GT3's ABS are now
  computed, not fed). `--feed aids` puts the recorded aid outputs back. Next: the `Car::step`
  shell (`spec_car_step.md` when the helper has written it; listings `cs_*.asm` are there).
- **Earlier**: aero is ported and committed (`car/aero.rs`: `AeroModel` slot, `VanillaAero` with
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
