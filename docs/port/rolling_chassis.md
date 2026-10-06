# Rolling chassis: body, suspension, steering and tyres on the Rust ODE (Task 08)

Code: `crates/rustyac-physics/src/car/` (new), two small additions to the existing crate
(`INIReader::getFloat3` in `data/ini.rs`, a state restore in `tyre/rig.rs`), `tools/chassis_compare`
(new), golden excerpts in `crates/rustyac-physics/tests/golden/chassis_*.chgold` with
`tests/chassis_golden.rs`. Sources: the disassembly of `acs.exe` 1.16.4 with `acs.pdb`
(`tools/disasm.py`), the maps in `docs/map/`, and three instruction-level briefs written for this
task by read-only helpers in the git-ignored `re/scratch/task08/` (`spec_bodies.md`,
`spec_suspension.md`, `spec_steering_setup.md`). Nothing in the game folder or the Ghidra project
was changed and no game process was touched. Brakes, engine, drivetrain, aero, aids and contacts
were not ported.

---

## 1. Plain-English summary

The Rust car can now stand on its wheels and roll. It has a body, a fuel tank and four wheel
carriers, each held by five rods as in the game, with springs, dampers, the third "heave" spring
of each axle, anti-roll bars, the steering rods and the number the game sends to the steering
wheel as force feedback, and the four Rust tyres from the earlier task sit on it. Everything is
built from the F2004's own data files, placed on the road the way the game places a car, and given
the same session start (medium tyres, setup values rounded the way the game's setup screen rounds
them). What is not written yet (the driver, engine, gearbox, brakes and wings) is taken from the
recordings of the game's own car: at each step the Rust car is told only what those parts did,
never where the car is or what its suspension or tyres did. It then has to drive the whole
recording on its own, from the first moment to the last, and every number is compared with the
game after every step. In all eleven recorded drives, 70,006 steps and up to a full minute of
driving, the Rust car matches the game in every digit: positions, speeds, the forces in every rod,
every spring, damper and anti-roll-bar push, about 300 numbers per tyre, the steering and the
force feedback. Because the real car never touches its bump stops in those drives, thirteen more
drives were recorded with the game's code and two altered copies of the car: one with its bump
stops and packers moved into reach, one with data-file values that make the game fall back to its
built-in defaults. Those 74,008 steps match in every digit too. To show that the comparison can
fail, eighteen deliberate faults (a single number changed in its last digit, or one rule left out)
were put into the Rust car one at a time, and each was noticed. A short piece of two drives (a
slalom and a kerb strike) is stored with the project's tests and runs without the game.

---

## 2. What was ported

### 2.1 Where the code lives, and why

`crates/rustyac-physics/src/car/`, a module of the existing physics crate rather than a new
`rustyac-car` crate: the tyre and its `Suspension` interface live in that crate, the chassis
needs the tyre and the rigid-body crate at the same time, and a third crate would only have
added a dependency edge without separating anything (`rustyac-physics` now depends on
`rustyac-ode`).

| File | Content |
|---|---|
| `body.rs` | `PhysicsCore` and the `RigidBodyODE` wrappers on `rustyac-ode`: bodies, distance and fixed joints, the rod reseat, and a force tape (every force call with its system, in call order, with the accumulators after it) |
| `suspension.rs` | the suspension slot `SuspensionModel`, `Damper`, `VanillaDwb` (the double wishbone) |
| `heave_spring.rs`, `antiroll_bar.rs` | `HeaveSpring`, `AntirollBar` |
| `setup.rs` | `SetupManager` for the items that point into chassis values, and the setup screen's round trip at a session start |
| `chassis.rs` | `RollingChassis`: construction, spawn, session start, the step in `Car::step` order, steering, force feedback |
| `feed.rs` | `ChassisFeed`: the hooks for the systems that are not ported |
| `replay.rs` | test rig only: the recorded feed, the recordings' road, the list of compared values, save / load of the chassis state, the golden-file format |

### 2.2 Traits and slots

- **`SuspensionModel`** (`suspension.rs`): AC's `ISuspension` with its 25 virtual functions
  (names from the PDB in snake_case, slot offsets in the comments), plus accessors for the shared
  members the setup items point into (`base()`: `k`, `progressiveK`, `rodLength`, `packerRange`,
  `bumpStopRate`, `staticCamber`, `toeOUT_Linear`, …). The rigid bodies live in the
  `PhysicsCore`, which is handed to every method that touches one. `RollingChassis::suspensions`
  is a `Vec<Box<dyn SuspensionModel>>`; heave springs, anti-roll bars, steering and setup only
  talk to the trait. `VanillaDwb` is the one implementation (STRUT, AXLE and ML cars are refused
  with a message).
- **The tyre's hub is now the real suspension.** `VanillaTyre` still sees the six-method
  `tyre::Suspension` it was written against; inside the chassis that is a thin adapter over a
  `SuspensionModel` and the rigid-body world. The single-wheel rig's fake hub is only used by
  the tyre's own tests any more.
- **`ChassisFeed`** (`feed.rs`): one hook per group of not-yet-ported systems, called where
  `Car::step` / `Car::stepComponents` run them: `poll_controls`, `engine` (the three numbers the
  fuel burn reads), `autoclutch`, `current_gear` (the sleeping rule reads it), `brakes`
  (positions 1 and 2), `aero` (6 to 9), `drivetrain` (11 to 14), `aids` (16 to 18), `stability`
  (21 to 29, after the setup items are written; the stability aid's yaw torque goes there).
  Every hook gets the chassis, so a real system can read what it needs and write where the
  game writes.
- `RayTrackCollisionProvider` and `TyreCar` (existing tyre interfaces) are implemented by the
  chassis side: the road is a boxed provider, the car adapter answers from the chassis.

### 2.3 Functions (addresses in `acs.exe`)

| Area | Functions |
|---|---|
| Rigid-body layer | `PhysicsCore::PhysicsCore` 0x1402cba80 (world settings), `createRigidBody` 0x1402cc3b0 with `RigidBodyODE::RigidBodyODE` 0x1402cd800, `PhysicsCore::step` 0x1402cd690 (the `dWorldStep` part), `createDistanceJoint` 0x1402cc190, `createFixedJoint` 0x1402cc2a0, `reseatDistanceJointLocal` 0x1402cd480, the joints' `setERPCFM` 0x1402cd5a0; `RigidBodyODE::getWorldMatrix` 0x1402ce6c0, `localToWorld` 0x1402ce780, `worldToLocal` 0x1402ceb30, `localToWorldNormal` 0x1402ce7c0, `worldToLocalNormal` 0x1402ceb70, `getPosition` 0x1402ce640, `getVelocity` 0x1402ce680, `getAngularVelocity` 0x1402ce3f0, `getLocalPointVelocity` 0x1402ce4e0, `getPointVelocity` 0x1402ce600, `getMass` 0x1402ce570, `setPosition` 0x1402ce9e0, `setRotation` 0x1402cea00, `setMassBox` 0x1402ce890, `stop` 0x1402cead0, `addForceAtPos` 0x1402cdf30, `addForceAtLocalPos` 0x1402cdee0, `addLocalForce` 0x1402cdf80, `addLocalForceAtPos` 0x1402ce010, `addLocalForceAtLocalPos` 0x1402cdfc0, `addTorque` 0x1402ce3d0, `addLocalTorque` 0x1402ce060 |
| Double wishbone | `Suspension::Suspension` 0x1402c0720, `loadINI` 0x1402c1d90, `attach` 0x1402c0fb0, `step` 0x1402c3390, `setSteerLengthOffset` 0x1402c3290, `setERPCFM` 0x1402c3230, `addForceAtPos` 0x1402c0bf0, `addTorque` 0x1402c0f10, `addLocalForceAndTorque` 0x1402c0d40, `getSteerBasis` 0x1402c1c00, `getHubWorldMatrix` 0x1402c4c50, `getPointVelocity` 0x1402c4d40, `getHubAngularVelocity` 0x1402c1bc0, `getVelocity` 0x1402c1d70, `getMass` 0x1402c4d30, `stop` 0x1402c6b20, `getBasePosition` 0x1402c4690, `getSteerTorque` 0x1402c1d60, `setDamage` 0x1402c31e0, `resetDamage` 0x1402c31d0, `getDamage` 0x1402c1500; `Damper::Damper` 0x1402b3250, `Damper::getForce` 0x1402b3280 |
| Heave springs, bars | `HeaveSpring::HeaveSpring` 0x14026daf0, `init` 0x1402b3310, `initData` 0x1402b3340, `step` 0x1402b3960, `Car::initHeaveSprings` 0x140273f40; `AntirollBar::init` 0x1402bb610, `step` 0x1402bb640, `Car::buildARBS` 0x14026f750 (without the optional controller files) |
| Body bookkeeping | the ported part of `Car::Car` 0x14026bf00 (creation order of bodies and joints), `Car::initCarData` 0x140272b30 (mass, inertia box, fuel, tank, steering and force-feedback keys), `Car::calcBodyMass` 0x14026fb70, `Car::updateBodyMass` 0x140276c70, `Car::getTotalMass` 0x140272570, `Car::isSleeping` 0x1402745e0, the C runtime's `rand()` for the four damage directions |
| Spawn | `Car::forceRotation` 0x140270040, `Car::forcePosition` 0x14026fe10 with `Car::reset` 0x1402758e0 (the chassis part), `RaceEngineer::getBaseCarHeight` 0x14027ba00 |
| The step | `Car::stepPreCacheValues` 0x1402768c0 (the cached speed), `Car::step` 0x140275da0 (joint softening below 1 m/s, fuel burn, steering signal, sleeping rule, body acceleration), the order of `Car::stepComponents` 0x1402764d0, `SteeringSystem::step` 0x1402b81b0 |
| Force feedback | `Car::getSteerFF` 0x140272180, `Car::onTyresStepCompleted` 0x140274cd0, `ksSawToothWave` 0x14022d2a0 |
| Setup | `SetupItem::SetupItem` 0x1402cb170, `SetupManager::init` 0x140289290, the chassis items of `SetupManager::initItems` 0x140289570 (bars, tyre pressures, per wheel dampers / bump-stop rate / spring / progressive rate / rod length / camber / toe / packer, the heave items, steer assist), `SetupManager::step` 0x14028d090 with its per-item body 0x140288b20 and the two rear-toe callbacks 0x140288f00 / 0x140288ee0; the session-start round trip of the setup screen (`SetupScreen::loadINI` 0x14017d950, `SetupTab::addItem` 0x140183850, the spinner job 0x140183620) as `tools/car_oracle` mirrors it |
| File reader | `INIReader::getFloat3` 0x1402357a0 with `getVector3` 0x140236540 |

### 2.4 Things in the machine code that decide bits

Found while reading, all confirmed by the comparison (each would have shown as a difference):

- **`IRigidBody::getVelocity` is not the stored velocity.** It is `dBodyGetRelPointVel(0,0,0)`,
  the velocity of the body's origin computed like any other point's. The value is the same, the
  sign of a zero is not always.
- **The steering reseat is a round trip.** `reseatDistanceJointLocal` turns both anchors from
  body to world coordinates and ODE turns them back, so even the untouched hub-side anchor
  changes in its last bits every step; then the rod gets its creation-time length back.
- **Only setup values that differ are written, and only at the end of the first step.** The
  session start therefore runs step 0's suspension, tyres and steering with the car file's
  values (camber 3.0° / 2.0°, front toe 0.00012, rear packer 0.060 m) and everything after with
  the rounded ones (2.9° / 1.9°, 0.0001, 0.059 m). An attached setup item also pulls its value
  back at the end of every step if anything else changes it.
- **The spawn matrix has negative zeros**, and the spawn height is
  `y + (baseCarHeight + 0.01)` with the radius of the tyre compound the constructor loaded.
- **The body-side force of a bump stop acts at the hub's current position**, the spring's and
  the damper's at the design position; the spring and bump stops are given in body axes, the
  damper in world axes.
- **The heave spring pushes each hub with the full force**, may pull, uses a fixed 500,000 N/m
  for its bump stops, and overwrites its own rod length every step with the mean of the two
  wheels' rod lengths.
- **The anti-roll bar normalises the body's up axis** (square root, one division, three
  multiplications); the wheel and heave code use it as it is.
- **Force feedback adds a constant 1.0** before the car's gain whenever the front wheels turn
  faster than 7 rad/s combined (the flat-spot term's `+ 1.0`), and the steer-assist curve is a
  `powf` of the game's C runtime.
- The fuel burn multiplies in double precision with the *single-precision* consumption factor
  widened; the tank's mass follows the fuel only once per 1000 ms of physics time, the first
  time at step 333 of a recording.
- Sums are written in the order of the instructions throughout (dot products start with the y
  term, cross products in the order of the code), comparisons as the branch instructions make
  them (they decide what a NaN does).

### 2.5 Not ported (by design of this task, or refused with a message)

Strut, axle and multilink suspensions; `[EXPLICIT_INERTIA]`; the controller files
`ctrl_arb_front.ini`, `ctrl_arb_rear.ini`, `ctrl_4ws.ini` (they need `DynamicController`); the
active-suspension branch of `Suspension::step` (nothing ever switches it on); the control lock,
gentle-stop and penalty overrides, lights, black-flag teleport, water temperature, collider
mask and jump-start parts of `Car::step`; `Car::postStep`; collision damage (the damage members
and `setDamage` exist, nothing calls them yet); the setup's `[RULES]` ride-height check; setup
items of systems that are not ported; the call to the driver's device (`sendFF`): `last_ff` is
the number it would be sent.

---

## 3. Results

### 3.1 Free run of every recording

`tools/chassis_compare run`: the Rust chassis is built from `cardata/ks_ferrari_f2004`, spawned,
given the session start, and then steps through the whole recording without ever being reset to
the game's state. After every step these are compared with the game, bit for bit (a NaN equals
a NaN):

- the six bodies: mass, inertia, position, quaternion, rotation matrix, linear and angular
  velocity at the entry of `dWorldStep` and after it, the force and torque accumulators at its
  entry, the place in the solver's island;
- the 21 joints: anchors, ERP, CFM, rod length (the fuel-tank joint: relative rotation and
  offset), the place in the solver's row order, and the constraint force on both bodies;
- per wheel: suspension travel, damper speed, steer torque, lower bump-stop limit; heave travel
  and bar rate per axle;
- per tyre: the 60 inputs as the tyre found them (hub matrix with camber, hub velocities, car
  speed, sleeping flag, ground hit …), all 220 outputs (force calls on the hub, load, slip,
  forces, radii, pressures, wear, the thermal core and its 36 surface patches) and the wheel's
  spin matrix;
- the car: steering signal, cached speed, body acceleration, fuel, force feedback (`lastFF`)
  and its filter state (`mzCurrent`), the sleeping counter;
- the force tape: every call that hands a force or torque to a body, in call order: body,
  kind, both vectors, the system it is booked under (spring, damper, bump stop, heave spring,
  heave damper, heave bump stop, anti-roll bar, tyre, wing, sleep), and the body's accumulators
  after it.

That is 2,009 named values per step plus about 65 force calls with 14 numbers each. The other
359 values of a recording's step are inputs (the script, the applied controls), constants of the
session, or belong to what is not ported: engine, drivetrain, brakes, wings, aids, water
temperature, the telemetry page, and the accumulators after ODE has added gravity (inside
`dWorldStep`; they are covered by the body states after the step).

<!-- RESULTS:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Values compared per step | Force calls compared |
|---|---|---|---|---|---|
| `brake` | 5334 | 100 % (5334/5334) | none | 2009 | 330544 |
| `kerb` | 4001 | 100 % (4001/4001) | none | 2009 | 260438 |
| `launch_autoclutch_off` | 4667 | 100 % (4667/4667) | none | 2009 | 302848 |
| `launch_autoclutch_on` | 4667 | 100 % (4667/4667) | none | 2009 | 304414 |
| `liftoff_oversteer` | 5334 | 100 % (5334/5334) | none | 2009 | 346178 |
| `random` | 20001 | 100 % (20001/20001) | none | 2009 | 1316458 |
| `settle` | 667 | 100 % (667/667) | none | 2009 | 37354 |
| `slalom` | 5334 | 100 % (5334/5334) | none | 2009 | 348436 |
| `steady_corner_120` | 6667 | 100 % (6667/6667) | none | 2009 | 436414 |
| `steady_corner_180` | 6667 | 100 % (6667/6667) | none | 2009 | 429784 |
| `steady_corner_60` | 6667 | 100 % (6667/6667) | none | 2009 | 436414 |
| **all** | **70006** | **100 % (70006/70006)** | | | **4549282** |

| Scenario | Wheel-steps on a packer | Bump-stop force calls | Axle-steps on a heave packer | Heave bump-stop force calls | Steps frozen by the sleeping rule | Wheel-steps with an idle spring |
|---|---|---|---|---|---|---|
| `brake` | 0 | 0 | 0 | 0 | 1748 | 150 |
| `kerb` | 0 | 0 | 0 | 0 | 257 | 150 |
| `launch_autoclutch_off` | 0 | 0 | 0 | 0 | 386 | 150 |
| `launch_autoclutch_on` | 0 | 0 | 0 | 0 | 257 | 150 |
| `liftoff_oversteer` | 0 | 0 | 0 | 0 | 257 | 1279 |
| `random` | 0 | 0 | 0 | 0 | 257 | 150 |
| `settle` | 0 | 0 | 0 | 0 | 512 | 150 |
| `slalom` | 0 | 0 | 0 | 0 | 257 | 150 |
| `steady_corner_120` | 0 | 0 | 0 | 0 | 257 | 150 |
| `steady_corner_180` | 0 | 0 | 0 | 0 | 257 | 3465 |
| `steady_corner_60` | 0 | 0 | 0 | 0 | 257 | 150 |
| **all** | **0** | **0** | **0** | **0** | **4702** | **6094** |
<!-- RESULTS:END -->

The second table counts how often the branches were taken that ordinary driving of this car
does not reach (see 3.2), the steps in which the sleeping rule froze the body, and the
wheel-steps in which a spring was fully extended and did not push.

`settle_floor` is left out as the task says: its floor contacts are stage 2 of the rigid-body
port. Its first 53 steps, up to the first step with floor contact joints, are identical too
(`chassis_compare run settle_floor --stop-after 53`); from step 53 on the Rust car has no floor
to stand on and leaves the recording, as it must.

The whole run gives the same result with `RUSTYAC_MATH=std`, i.e. with Rust's own `sinf`,
`cosf` and `powf` in place of the game's C runtime (an observation on these recordings, not a
guarantee).

### 3.2 Two altered copies of the car, for what the real one never reaches

In the eleven drives above no wheel ever reaches a packer or a bump stop and no heave spring its
packer or bump stops (the second table of 3.1 counts them: all zero), so those branches would
have been ported blind. `chassis_compare test-car` therefore writes a copy of the car,
`cardata/f2004_tight_stops`, that differs from the F2004 in 18 values of `suspensions.ini` and
`setup.ini`: front / rear `BUMPSTOP_UP` 0.028 / 0.068, `BUMPSTOP_DN` −0.016 / −0.052 (a negative
value puts the lower stop above the design position), `PACKER_RANGE` 0.020 / 0.030, a
`BUMP_STOP_PROGRESSIVE` of 2,000,000 / 1,000,000 (a key the F2004 does not have), the heave
springs' `BUMPSTOP_UP` 0.027 / 0.067, `BUMPSTOP_DN` −0.017 / −0.051, `PACKER_RANGE`
0.012 / 0.025, and the setup screen's lower limit for the four packers (5 mm, so that it does
not push them back). The game's own code then drove that car through eight of the scenarios
(`car_oracle all --car f2004_tight_stops --out oracle/car_tight_stops`, each twice with equal
hashes), and the Rust chassis, built from the same files, was compared the same way:

<!-- TIGHT:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Values compared per step | Force calls compared |
|---|---|---|---|---|---|
| `brake` | 5334 | 100 % (5334/5334) | none | 2009 | 352674 |
| `kerb` | 4001 | 100 % (4001/4001) | none | 2009 | 271680 |
| `launch_autoclutch_on` | 4667 | 100 % (4667/4667) | none | 2009 | 329834 |
| `liftoff_oversteer` | 5334 | 100 % (5334/5334) | none | 2009 | 376426 |
| `random` | 20001 | 100 % (20001/20001) | none | 2009 | 1356128 |
| `settle` | 667 | 100 % (667/667) | none | 2009 | 42356 |
| `slalom` | 5334 | 100 % (5334/5334) | none | 2009 | 364010 |
| `steady_corner_180` | 6667 | 100 % (6667/6667) | none | 2009 | 476872 |
| **all** | **52005** | **100 % (52005/52005)** | | | **3569980** |

| Scenario | Wheel-steps on a packer | Bump-stop force calls | Axle-steps on a heave packer | Heave bump-stop force calls | Steps frozen by the sleeping rule | Wheel-steps with an idle spring |
|---|---|---|---|---|---|---|
| `brake` | 24 | 13380 | 1712 | 8464 | 1739 | 10 |
| `kerb` | 29 | 5276 | 55 | 5428 | 227 | 10 |
| `launch_autoclutch_on` | 24 | 11090 | 5048 | 13792 | 227 | 10 |
| `liftoff_oversteer` | 3317 | 22024 | 1511 | 5428 | 227 | 10 |
| `random` | 322 | 28812 | 3902 | 10320 | 227 | 10 |
| `settle` | 24 | 2676 | 47 | 1788 | 482 | 10 |
| `slalom` | 24 | 9608 | 47 | 5428 | 227 | 10 |
| `steady_corner_180` | 7337 | 34492 | 1327 | 5428 | 227 | 10 |
| **all** | **11101** | **127358** | **13649** | **56076** | **3583** | **80** |
<!-- TIGHT:END -->

The car is not meant to be realistic (it rides on its stops most of the time); it is the game's
code on other numbers. By call site, the recordings contain both the upper and the lower bump
stop of all four wheels and of both heave springs.

The second copy, `cardata/f2004_fallbacks`, changes 16 values so that the loaders take branches
the F2004's files never trigger, and two code paths of steering and force feedback run:
`suspensions.ini` `VERSION=1` (no rim offset is applied), front `HUB_MASS=0` (becomes 20 kg),
fast damper rates and thresholds of 0 on wheels and heave springs (become the slow rate and
0.2 m/s), `BUMP_STOP_RATE=0` (becomes 500,000 N/m), a rear `TOE_OUT` of −0.00006 (off the
setup screen's grid, so the session start changes the rear toe and the setup item's callback
reseats the rear steering rods); `car.ini` `STEER_ASSIST=1` (the force feedback takes its
linear path instead of the `powf` curve), `LINEAR_STEER_ROD_RATIO=0` (becomes 0.003), `FUEL=0`
(becomes 30 litres) and a `[FUEL_EXT] KG_PER_LITER` of 0.76. Its session start changes 21 setup
values instead of 11 (several of the defaults are outside the setup screen's ranges and are
clamped). Five scenarios (`car_oracle all --car f2004_fallbacks --out oracle/car_fallbacks`,
each twice with equal hashes):

<!-- FALLBACKS:BEGIN -->
| Scenario | Steps | Bit-exact steps | First divergence | Values compared per step | Force calls compared |
|---|---|---|---|---|---|
| `brake` | 5334 | 100 % (5334/5334) | none | 2009 | 328922 |
| `kerb` | 4001 | 100 % (4001/4001) | none | 2009 | 260338 |
| `settle` | 667 | 100 % (667/667) | none | 2009 | 37256 |
| `slalom` | 5334 | 100 % (5334/5334) | none | 2009 | 348338 |
| `steady_corner_180` | 6667 | 100 % (6667/6667) | none | 2009 | 429926 |
| **all** | **22003** | **100 % (22003/22003)** | | | **1404780** |

| Scenario | Wheel-steps on a packer | Bump-stop force calls | Axle-steps on a heave packer | Heave bump-stop force calls | Steps frozen by the sleeping rule | Wheel-steps with an idle spring |
|---|---|---|---|---|---|---|
| `brake` | 0 | 0 | 0 | 0 | 1883 | 158 |
| `kerb` | 0 | 0 | 0 | 0 | 265 | 158 |
| `settle` | 0 | 0 | 0 | 0 | 520 | 158 |
| `slalom` | 0 | 0 | 0 | 0 | 265 | 158 |
| `steady_corner_180` | 0 | 0 | 0 | 0 | 265 | 3353 |
| **all** | **0** | **0** | **0** | **0** | **3198** | **3985** |
<!-- FALLBACKS:END -->

### 3.3 Can the comparison fail?

`chassis_compare faults`: a free run again, each time with one deliberate fault in the Rust
chassis; once on a recording of the real car and once on one of the tight-stops car
(`chassis_compare faults random --dir oracle/car_tight_stops`). "One bit up" is the next
representable number.

<!-- FAULTS:BEGIN -->
Real car: scenario `slalom` of the car `ks_ferrari_f2004`, 5334 steps.
Tight-stops car: scenario `random` of the car `f2004_tight_stops`, 20001 steps.
Without a fault neither run shows a difference.

| Fault | What is changed | Real car: noticed at | Tight-stops car: noticed at | First value that differs (where it is noticed first) |
|---|---|---|---|---|
| `damper` | left front slow bump damping one bit up | step 2 | step 4 | force call 0 (addForceAtPos on hub_lf, damper): a[0]: game -1.0773937e-9 (0xb094136c) / Rust -1.0773938e-9 (0xb094136d) |
| `spring` | right rear spring rate one bit up | step 35 | step 4 | force call 8 (addForceAtPos on hub_rr, spring): a[0]: game -1.5559265e-9 (0xb0d5d84c) / Rust -1.5559267e-9 (0xb0d5d84e) |
| `rod` | left rear rod length one bit up (in magnitude) | step 0 | step 0 | suspension.lr.travel: game -0.039999988 (0xbd23d707) / Rust -0.03999999 (0xbd23d708) |
| `bump_stop_rate` | right front packer / bump stop rate one bit up | never | step 0 | force call 6 (addForceAtPos on hub_rf, bumpstop): a[1]: game 1888.0007 (0x44ec0006) / Rust 1888.001 (0x44ec0008) |
| `packer` | left front packer range one bit up | never | step 55 | force call 0 (addForceAtPos on hub_lf, spring): a[0]: game -6.528908e-5 (0xb888ebcf) / Rust -5.5180302e-5 (0xb8677166) |
| `bump_stop_up` | left rear upper bump stop one bit further | never | step 6 | force call 12 (addForceAtPos on hub_lr, bumpstop): a[0]: game 1.4120957e-7 (0x34179f64) / Rust 1.412087e-7 (0x34179f27) |
| `bump_stop_dn` | right rear lower bump stop one bit further | step 0 | step 0 | suspension.rr.bumpStopDn: game -0.2 (0xbe4ccccd) / Rust -0.20000002 (0xbe4cccce) |
| `heave_packer` | front heave spring packer range one bit up | never | step 52 | force call 28 (addForceAtPos on hub_lf, heave_spring): a[0]: game -0.00019643846 (0xb94dfb0c) / Rust -0.00019643843 (0xb94dfb0a) |
| `heave_bump_stop` | rear heave spring lower bump stop one bit further | never | step 0 | force call 40 (addForceAtPos on hub_lr, heave_bumpstop): a[1]: game 25499.994 (0x46c737fd) / Rust 25499.996 (0x46c737fe) |
| `arb` | front anti-roll bar rate one bit up | step 0 | step 0 | arb.front.k: game 40000.0 (0x471c4000) / Rust 40000.004 (0x471c4001) |
| `heave` | rear heave spring rate one bit up | step 0 | step 0 | force call 24 (addForceAtPos on hub_lr, heave_spring): a[1]: game 3583.999 (0x455ffffc) / Rust 3583.9993 (0x455ffffd) |
| `heave_damper` | front heave damper slow rebound one bit up | step 63 | step 6 | force call 32 (addForceAtPos on hub_lf, heave_damper): a[0]: game 4.503248e-7 (0x34f1c42b) / Rust 4.5032482e-7 (0x34f1c42c) |
| `steer_ratio` | steering rod ratio one bit up | step 2495 | step 687 | joint.lf.steer_rod.anchor1.x: game 0.13739617 (0x3e0cb195) / Rust 0.13739616 (0x3e0cb194) |
| `ffmult` | force-feedback gain one bit up | step 46 | step 47 | car.lastFF: game 7.038664e-7 (0x353cf155) / Rust 7.038665e-7 (0x353cf156) |
| `fuel_kg` | fuel density one bit up | step 333 | step 333 | body.post.lvel.y: game -0.00059753243 (0xba1ca3b9) / Rust -0.00059753313 (0xba1ca3c5) |
| `consumption` | fuel consumption factor one (single-precision) bit up | step 409 | step 409 | car.fuel: game 79.9999510372482 (0x4053ffff32a2a544) / Rust 79.9999510372425 (0x4053ffff32a2a3b3) |
| `no_setup_rounding` | setup-screen rounding left out (camber, toe, packers as in the car's files) | step 1 | step 1 | body.post.pos.x: game 2.6926699e-12 (0x2c3d7ad4) / Rust 3.769761e-12 (0x2c84a2fd) |
| `erp` | the low-speed softening of the joints left out | step 0 | step 0 | body.post.pos.x: game 2.1541822e-12 (0x2c17964c) / Rust 1.6156367e-12 (0x2be36172) |
<!-- FAULTS:END -->

All eighteen are noticed on the tight-stops car. On the real car five are not (packer range,
bump-stop rate, upper bump stop, heave packer, heave bump stop), which is the gap 3.2 closes;
the lower bump stop is noticed there only because the limit itself is one of the recorded
values. The command ends with an error naming the faults a recording does not notice.

The first version of this check put the faults in after the chassis was built and five others
went unnoticed. The reason was not the comparison: the attached setup items wrote the setup's
values back at the end of step 0, exactly as they do in the game, and undid the fault. The
faults now detach the item first.

### 3.4 Golden test in `cargo test`

`crates/rustyac-physics/tests/chassis_golden.rs` replays two excerpts of 300 steps
(`tests/golden/chassis_slalom_3000_300.chgold`, `chassis_kerb_2684_300.chgold`, 321 kB each,
written by `chassis_compare excerpt`): the chassis state at the start (bodies, joints, tyres,
counters; saved from the Rust free run at that step, which is identical to the game's), per step
the feed, and per step the game's answer: position, quaternion and velocities of the six bodies
in full and one hash over the 2,009 values and the force tape of 3.1. The slalom excerpt is
100 km/h with the steering swinging; the kerb excerpt starts 40 steps before the left front
wheel climbs the 2 cm strip. A second test changes one input in its last bit (a steering value,
a wing force, the start height of a hub) and requires the replay to fail at that step. All of it
also passes with `RUSTYAC_MATH=std` (Rust's own maths in place of the game's C runtime).

The excerpts hold no car parameters: the test loads them from `cardata/ks_ferrari_f2004`
through the ported loaders. That folder is extracted game data and not in git; without it the
replay tests print `NOT TESTED` and pass (see 6.1).

---

## 4. What the feed still supplies, i.e. what comes next

Per step the recorded feed (`replay::RecordedStep`, filled by `tools/chassis_compare` from a
recording) hands the chassis exactly this, and nothing the chassis computes itself:

| Fed value | From the recording | Who produces it in the game | Task that removes it |
|---|---|---|---|
| steer, gas, brake, clutch as the device reports them | `script.*` | the driver's device (`Car::pollControls`) | stays an input (driver / AI) |
| the clutch after the automatic clutch | `controls.clutch` | `Autoclutch::step` | drivetrain |
| current gear (only read by the sleeping rule) | `drivetrain.currentGear` of the step before | gearbox | drivetrain |
| engine speed, throttle the engine used, turbo boost (only for the fuel burn) | `drivetrain.engineRPM`, `engine.gasUsage`, `engine.status.turboBoost` of the step before | `Engine::step` | engine |
| per tyre: brake torque, handbrake torque | `tyre.<w>.in_brake_torque`, `in_hand_brake_torque` | `BrakeSystem::step`, `EDL::step` | brakes |
| per tyre: electric torque, ABS factor, AI grip factor | `in_electric_torque`, `in_abs_override`, `in_ai_mult` | ERS, `ABS::step`, AI driver | hybrids / aids / AI |
| driven wheels: wheel speed and spin matrix | `in_set_av_value`, `in_localWheelRotation.*` | `Drivetrain::step2WD` + `Tyre::stepRotationMatrix` | drivetrain |
| the wings' pushes on the body (14 calls per step once the car moves) | force-tape entries booked `aero_drag` / `aero_lift` | `AeroMap::step`, `Wing::step` | aero |
| air and road temperature, track grip level, wear and damage rates, tyre blankets | constants of the recording | `PhysicsEngine`, `Track` | session / track |

Before the first step the engine has not run: gear 1 (neutral) and zero throttle use are fed
(the recording has no step before the first; with a throttle use of zero the engine speed does
not matter to the fuel burn). The recordings hold the tyre's inputs as `Tyre::step` found them,
so the recorded feed writes everything of rows 5 to 7 in its `brakes` hook, right before the
suspensions and tyres run; its `drivetrain`, `aids` and `stability` hooks are empty.

Suggested order for what follows (as in `docs/map/car.md` section 3): brakes (small, removes
rows 5 and part of 6), then engine and drivetrain (rows 2 to 4 and 7; the first point where the
driven wheels close their loop in Rust), then `DynamicController`, aero (row 8) and the aids.
The feed trait already has a hook at each of their positions.

---

## 5. Commands

```
cargo test -p rustyac-physics                      # includes the chassis golden test (needs cardata/ks_ferrari_f2004)
cargo build --release --manifest-path tools/chassis_compare/Cargo.toml

chassis_compare run                                # all recordings in oracle/car but settle_floor; writes oracle/chassis/results.md
chassis_compare run slalom kerb --verbose          # some; prints the first 40 differing values; writes partial.md
chassis_compare run --stop-after 500               # only the first steps of each
chassis_compare faults [<scenario>] [--dir <folder>]   # the deliberate-fault check (default slalom); writes oracle/chassis/faults.md
chassis_compare excerpt                            # rewrite the two golden excerpts

chassis_compare test-car                           # write cardata/f2004_tight_stops and cardata/f2004_fallbacks
car_oracle all --car f2004_tight_stops --out oracle/car_tight_stops --only settle,launch_autoclutch_on,brake,steady_corner_180,slalom,liftoff_oversteer,kerb,random
car_oracle all --car f2004_fallbacks --out oracle/car_fallbacks --only settle,brake,slalom,kerb,steady_corner_180
chassis_compare run --dir oracle/car_tight_stops   # writes oracle/chassis/results_car_tight_stops.md
chassis_compare run --dir oracle/car_fallbacks     # writes oracle/chassis/results_car_fallbacks.md
chassis_compare faults random --dir oracle/car_tight_stops   # writes oracle/chassis/faults_car_tight_stops.md
```

(Run from the repository folder; `chassis_compare` stands for
`tools/chassis_compare/target/release/chassis_compare.exe`, `car_oracle` for
`tools/car_oracle/target/release/car_oracle.exe`. `chassis_compare` needs only the recordings
that `car_oracle all` makes and the car's data folder, no `acs.exe`; `car_oracle` needs the
game's `acs.exe` and the Visual C++ 2013 runtime as before. `run` and `faults` end with an
error if a recording differs or a fault goes unnoticed. The whole `run` takes about six
seconds.)

---

## 6. Open questions

### 6.1 Choices made while working unattended

1. **A module, not a crate** (2.1).
2. **The recordings were not changed.** Everything the feed needs was already in them, so the code
   of `tools/car_oracle` was not touched (its `Cargo.lock` gained the physics crate's new
   dependency) and the twelve recordings and their hashes are the ones of Task 06. Two values are
   taken from the step before (engine values, gear) because the game reads them before the engine
   and gearbox run; for step 0 they are the constructor's (zero throttle use, neutral). That only
   the automatic clutch rewrites the clutch pedal between the device and the sleeping rule was
   checked in the decompiled writers of `controls.clutch`.
3. **Two altered cars** (3.2) instead of new scenarios for the real one: the F2004 on a flat
   road cannot reach its stops without leaving the ground (and its floor has no contact in the
   oracle), and no scenario can make a loader take a fall-back. The copies live in the
   git-ignored `cardata/` and are written by a command, their recordings in the git-ignored
   `oracle/car_tight_stops/` and `oracle/car_fallbacks/`. Their numbers are arbitrary. The old
   scenario names, recordings and the determinism check are untouched; `car_oracle` already had
   the `--car` and `--out` options this needs.
4. **The golden test needs the car's data folder** (3.4). The alternative, storing every loaded
   parameter in the excerpt, would have put the car's data into git in another form. Without
   the folder the two replay tests pass without testing; they say so on standard error.
5. **The golden start state is the Rust state**, saved after a free run that was compared with
   the game at every step up to there, not a state rebuilt from the recording's columns. The
   recording does not hold everything (the flat-spot phase of the force feedback, the time of
   the last mass refresh), the Rust state does.
6. **Tyre inputs are written in one hook** (section 4), because that is how they were recorded.
   The hooks for the drivetrain and the aids exist and are called at the game's positions.
7. **`rand()` is emulated** (the Visual C++ generator: `state * 214013 + 2531011`, bits 16 to
   30), seeded with the recording's seed, one draw per suspension. With no damage the drawn
   direction only decides the sign of a zero that is added to other terms; a different seed
   gives the same recordings (not checked beyond the two seeds the recordings use).
8. **The harness's ray length**: the oracle's fake road answers a hit up to 3 m below the ray's
   origin whatever length the tyre asks for (2 m), and the Rust road copies that.
9. **Formatting**: the existing sources are not `rustfmt`-clean under any setting tried, so the
   new files were formatted by hand in the style of their neighbours; `cargo clippy` is clean.
10. **Files outside the repository.** One of the read-only helpers reported that its first
    command wrote a temporary listing into the system temp folder and that it deleted it at
    once (rule 3 says scratch work belongs in `re/scratch/`). The agent harness itself keeps
    the console output of background commands and over-long tool outputs in its own folders
    (system temp, the user's `.claude` folder), and the tooling notes for later sessions, which
    live in that `.claude` folder, got one new entry about `chassis_compare`. No tool of this
    task writes outside the repository.

### 6.2 Questions

1. **Branches no recording reaches**, ported from the disassembly only: the flat-spot shake of
   the force feedback with a flat spot present (in every step with turning front wheels the
   flat-spot level is 0, so only its `+ 1.0` is exercised), the gyro term with a non-zero gain,
   a force-feedback filter other than 0, the "fake understeer" lightening (these three are
   settings of the driver's device and the game's ini file, which the oracle keeps at the
   shipped values), collision damage bending a steering rod, `addLocalForceAndTorque` (the game
   never uses that torque mode), a tyre whose ray misses the road, surface drag, loader
   fall-backs other than those of 3.2, and every NaN route. A read-only review of exactly
   these against the disassembly was run at the end of the task; its findings are in 6.3.
2. **Other cars.** Everything was compared on the F2004 and its two altered copies. Another
   double-wishbone car without controller files should load, but none was recorded; 30 of the
   113 cars need the strut (or axle) suspension first.
3. **The first second.** The game skips collision detection for the first 250 steps of a
   session; the oracle has no floor at all (Task 06, 4.4). The chassis inherits that: it is
   exact for a car whose floor never touches the road.
4. **Where should `Car::step`'s leftovers go** (control lock and penalties, water temperature,
   collider mask, jump start)? They are small, belong to no system and were left for the
   `Car::step` shell of the port plan (item 10).
5. **`car_oracle check`** still replays the tyres through the single-wheel rig. With the
   chassis comparison in place that check is a subset of this one; it was left as it is.
6. **The rest of the setup screen at a session start.** The oracle (Task 06) mirrors what the
   screen does to the `SetupManager` items. Its other parts (the fuel from `setup.ini [FUEL]`,
   electronics, gear sets) were not looked at then and not here; if the real game also sets the
   fuel at that moment, a real session starts with another tank mass than the recordings.
7. **The C runtime.** `rand()` is emulated from the generator's well-known formula, not from a
   disassembly of `MSVCR120.dll`; `_fdtest`'s return codes likewise. `powf`, `sinf` and `cosf`
   are the DLL's own when it is installed.
8. **A setup-screen cast in `tools/car_oracle`.** The game's float-to-integer conversions in the
   setup screen give the smallest integer for a NaN or an overflow; the harness uses Rust's
   saturating casts (6.3). No car in `cardata` reaches the difference, so the harness and its
   recordings were left alone; the port does what the game does.

### 6.3 Review of the untested branches

<!-- REVIEW:BEGIN -->
Three read-only reviewers compared the finished port with the disassembly, each on a part of
the code and with the list of what the recordings never reach. What they found, and what was
done:

| Files | Result |
|---|---|
| `suspension.rs`, `heave_spring.rs`, `antiroll_bar.rs` | No difference. Checked instruction by instruction: packer and both bump-stop blocks of wheel and heave spring (conditions, NaN routing, force expression, the point each force acts on, call order), `addLocalForceAndTorque`, the damage functions, the steering reseat with damage and with a wheel at x = 0, every loader fall-back, the second `attach`, all four damper branches, the anti-roll bar with a rate of 0 or below |
| `chassis.rs`, `body.rs`, `feed.rs` | One difference: `Car::updateBodyMass` takes its "explicit inertia" path whenever `[BASIC] INERTIA` is (0,0,0) or missing, section or not (the body then weighs the full `TOTALMASS`); the port took the box path. **Fixed**: such a car is now refused with a message, like one with `[EXPLICIT_INERTIA]`. Two notes: the stability aid runs after `SetupManager::step`, not with the other aids (a `stability` hook was added at that place), and the sleeping counter wraps in the game where the port would have stopped a debug build after 2^31 quiet steps (now wraps). Everything else matches, including all of `getSteerFF` with a flat spot, a gyro gain, a filter, steer assist 1 and NaN inputs, the five cases of the sleeping rule, the spawn, the construction order and every wrapper the drives never call |
| `setup.rs`, `replay.rs`, `get_float3`, the tyre state restore | Two differences. (1) `INIReader::getVector3` splits the first two numbers on commas but takes the rest of the line as the third (its third `wcstok_s` call uses line ends as delimiters), so `1,2,,3` makes the game throw where the port returned (1, 2, 3). **Fixed**, with a test. (2) The setup screen's float-to-integer conversions give the smallest integer for a NaN or an overflow; the port (like the oracle harness it mirrors) used Rust's saturating casts. The two differ only for a spinner in "clicks" mode 1 whose `STEP` reads as 0, or a NaN value. **Fixed in the port**; see 6.2 item 8 for the harness. All 67 chassis setup items (name, target, multiplier bits, order, conditions), `SetupManager::step`, the rear-toe callbacks, the state save / restore (nothing that changes from step to step is missing) and the road match |

None of the fixes changes a compared value: the three sets of recordings (3.1, 3.2) and the
tests were run again afterwards with the same result.

The reviewers could not check: the C runtime's internals (6.2 item 7), the bodies of functions
in other reviewers' files (covered there), and whether a real session pushes every setup item
through its spinner (Task 06 compared the resulting log lines for one car with a real game log).
<!-- REVIEW:END -->
