# Task 24: several cars on track

> **Task 38c:** the `re/scratch/task…` paths below are history: those folders were scratch and
> most were deleted in the disk cleanups of 2026-10-15 and 2026-10-18. Every proof of this report
> reruns from `tools/proofs` (`sh tools/proofs/run_all.sh`); `docs/port/proofs.md` lists each path
> and what replaces it.

## Resume here

**Task 24 is finished at v0.24.0** (steps 1 to 4, 6 and 7; step 5 was left for a Task 24b, see "Not done"). Nothing is half-way.

- **Done and proven against the game:** several cars in one world (`crates/rustyac-physics/src/engine/`),
  car-vs-car contacts and damage, the pit-lane rule, the physics thread pool, the other cars' drivers
  in `rustyac.exe` (`--cars`, `--opponent`, `--physics-threads`), the golden test, the performance numbers.
- **Not done (a Task 24b):** drawing and hearing the other cars (step 5 of the task), race.ini's
  `[CAR_n]` cars in `rustyac.exe`, the "remote" stand-in provider (its design is in section 7).
- To re-run a proof: `tools/car_oracle/target/release/car_oracle.exe multi --scenario mc_rear_end --track spa`
  (records the game's own engine, then replays the port with 0 and 8 worker threads).
- Specs read from the machine code and the three code reviews: `re/scratch/task24/spec_*.md`,
  `review_*.md` (git-ignored). Batches: `re/scratch/task24/more_proofs.sh`, `final_check.sh`.

## Plain-English summary

Until now rustyAC had one car alone on the track. This task puts **several cars** into the same world,
the way Assetto Corsa itself does it, and proves that the result is the game's, bit for bit.

- **One world for all cars.** The game keeps every car's body, wheels and suspension rods in one
  rigid-body world. Each step it first lets every car work out its own forces (engine, tyres, wings,
  springs), then looks once for things that touch, then moves the whole world forward by 3 ms. rustyAC
  now does exactly that, in the same order.
- **Cars touch each other.** Two cars that meet push each other apart and get damaged. The rules are the
  game's own, including two oddities that were copied as they are:
  - of the two cars in a contact, only the one that sees the other coming closer takes the damage;
  - a contact "bounces" above a speed the game never sets: it uses whatever number happened to be in
    memory. It was measured: it is a number so small (about 7e-43 m/s) that it acts as zero.
- **The pit lane.** A car in the pit lane cannot touch other cars: it drives through them. It becomes
  solid again once it has left the lane and no other car is within 6 m. Ported and checked.
- **Several processor cores.** The game works out the cars' forces on several threads when the PC has
  four or more logical processors. rustyAC does the same (`--physics-threads`, default as the game:
  half the logical processors). **The result never depends on the number of threads**: every proof
  below was run with one thread and with nine (the main one and eight helpers) and gave the same bits.
- **Proof.** A new oracle mode runs the game's own physics with 2 to 12 cars and writes down
  everything after every step. rustyAC then runs the same cars from step 0, given only what each
  driver did with the pedals and the wheel, and has to arrive at the same numbers every step.
  It does, in all 41 runs: 319 million numbers compared, none different. The runs include two cars
  side by side, a rear-end hit, a side swipe, a four-car pile-up into a barrier, the pit-lane rule,
  six cars for a whole lap of Spa, twelve cars at once, and mixed cars (Formula 1, road cars,
  four-wheel drive, live axle, rear-wheel steering, hybrids) on three tracks.
- **Other cars in the game.** `rustyac.exe --cars 6` puts five more cars on the grid; `--opponent`
  names each one and how it is driven: parked, by rustyAC's simple line follower, or by a recorded
  drive. These are stand-ins until real AI drivers exist (Task 25).
- **What is missing.** The other cars are **not drawn and not heard yet** by AC's renderer and sound
  (that part of the task was left for a Task 24b, as the task allows, so that the physics could be
  finished and proven). They can be seen as blue boxes in the debug view (`--debug-view`).
- A drive with **one car is unchanged**: it runs the same code as before, and every earlier proof and
  test still passes.

## 1. What is ported

| Part | In AC (`acs.exe`) | In rustyAC |
|---|---|---|
| The step for N cars: `Track::step`, wind, every car's `stepPreCacheValues` + `step`, one collision pass, one `dWorldStep`, the step-completed handlers (loose objects, then `Car::postStep` per car) | `PhysicsEngine::step` 0x140264760 | `engine::PhysicsEngine::step` |
| Creation order and `physicsGUID` (= number of cars already there; 0 is the player's: `[CAR_0]`) | `Car::Car` 0x14026bf00 (GUID at 0x14026c87b), `RaceManager::initOffline` 0x14013a6c0, `Sim::addCar` 0x1401978a0 | `PhysicsEngine::build_car`, `RollingChassis::new_in_core`, `RollingChassis::physics_guid` |
| Collision sub-space `GUID + 1` per car, category 4, masks | `CarColliderManager::loadINI` 0x1402a37a0, `Car::initColliderMesh` 0x140273b20, `PhysicsCore::getDynamicSubSpace` 0x1402cc920 | `install_box_colliders`, `init_collider_mesh` (the constant 0 became the car's GUID) |
| Car-vs-car detection: everything in `spaceDynamic` against itself on even frames, against the track on odd frames; pairs by the bit masks; at most 4 contacts between two bodies | `PhysicsCore::collisionStep` 0x1402cbf90, `nearCallback` 0x1402ccc70 | `PhysicsCore::collision_step` (unchanged: it was written for this in Task 13 and now runs with several sub-spaces) |
| Mesh-vs-mesh material: mode 0x7014, mu 0.25, bounce 0.01, soft CFM 1e-4, and the unwritten `bounce_vel` | `PhysicsCore::onCollision` 0x1402ccda0 | `PhysicsCore::on_collision`, `mesh_bounce_vel` |
| Every car is told of every new contact; the pair is turned round when only the second body is a car's | `PhysicsEngine::onCollisionCallBack` 0x140264020 | `chassis::turn_to_car`, the loop in `PhysicsEngine::step` |
| Damage, last-collision times, the collision event per car (`param1` = GUID) | `Car::onCollisionCallBack` 0x140274650 | `RollingChassis::on_collision_callback`, `CollisionInputs` |
| The pit-lane rule (mask 0x1a instead of 0x1e while "off for pits"; cleared outside the lane when no car is within 6 m) | `Car::updateColliderStatus` 0x140276df0 | `update_collider_status` (was there; now it gets the other cars' positions) |
| Slipstream: each car's wake, read by the others | `Car::updateAirPressure` 0x140276ae0, `SlipStream::getSlipEffect` 0x1402aac60, `setPosition` 0x1402aae80 | `update_air_pressure` (was there; now it gets the other cars' wakes) |
| The track's grip from the laps of **all** cars | `Track::step` 0x140278d20 | `PhysicsEngine::dynamic_track` |
| Joint softness by speed for car 0 only | `Car::step` 0x140275e1a | `env.is_first_car` (true for GUID 0) |
| The thread pool: size rule, which cars go to workers | `PhysicsEngine::PhysicsEngine` 0x140262430, `ThreadPool::ThreadPool` 0x1402d04a0, `addTask` 0x1402d0f30, `WorkerThread::operator()` 0x1402d0a00 | `engine::worker_threads`, `engine::pool::ThreadPool`, `PhysicsEngine::step_cars` |
| No contacts for 250 steps at a session's start | `PhysicsEngine::setSessionInfo` 0x140264560 | `PhysicsEngine::reset_collisions_for_new_session` |

### 1.1 What the machine code says (the answers to the maps' open questions)

- **Which car is GUID 0?** The player's. `Car::Car` takes the number of cars the engine already has;
  `RaceManager::initOffline` creates `[CAR_0]`, `[CAR_1]`, ... in order. Only GUID 0 switches its
  joints' ERP with speed, checks the jump start, and runs the lap invalidator, the telemetry, the
  performance meter and the penalty manager.
- **The step.** `stepPreCacheValues` runs for all cars first, on the physics thread. Then `Car::step`:
  without a pool, car after car; with a pool of W workers the physics thread itself steps the first
  `cars / W + 1` cars (so car 0 is always on the physics thread), every other car is one task, and the
  physics thread spins until all tasks are done. Everything after that is single-threaded.
- **The pool's size.** `[PHYSICS_THREADING] THREADS` of `system/cfg/assetto_corsa.ini`: no pool below
  4 logical processors or with 0; with -1 (the shipped value) half the logical processors; else the
  number itself. A file without the section reads as -1, a section without the key as 0. `THREADS=1`
  makes the game throw on every step (its split asks for a negative number of tasks); rustyAC simply
  runs with one worker.
- **Can threads change the game's result?** Not for cars driven by a person, a recording or a script:
  a car's step writes only its own car, and what it reads of others (their positions for the pit rule,
  their wakes) is not written during that phase. Two exceptions, both the game's own: a black-flag
  teleport inside `Car::step` can be seen half-done by another car's pit-lane test, and AI drivers
  call `rand()`, whose state is per thread (Task 25 will have to deal with that). The game with its
  own pool of 4 threads was run in the oracle and gave the same numbers as without a pool
  (section 3).
- **The contact groups** are swapped as the map said (`contactGroup` for car-vs-car frames,
  `contactGroupDynamic` for car-vs-track frames); copied as found in Task 13.
- **`bounce_vel`**, measured: the `dContact` is built in `onCollision`'s stack frame and the field is
  never written. For every mesh contact (car against car, wall or loose object) the joint gets the
  **upper 32 bits of the address of the first shape's geom**, read as a float. In the recordings:
  0x000001c5 to 0x0000028c, that is 6e-43 to 9e-43, the same for every joint of a run (all geoms are
  on the same part of the heap). ODE bounces when the closing speed is above it, so it works as 0.
  The oracle writes the value into the recording and the port uses it, so the contact joints match
  completely; `rustyac.exe` uses +0, which differs only for a closing speed below 1e-42 m/s.
- **Damage between two cars.** Both cars' callbacks get the same contact with the same normal, so
  the closing speed one car computes is the negative of the other's: only one of the two gets
  damage from a given contact point (which one depends on which shape ODE lists first). Both push an
  event for the sounds. Ported as it is.

## 2. How it is built

The game's `Car::step` writes straight into the one shared world, each car into its own bodies; that
is what lets it run the cars on several threads. In Rust two threads may not hold the same world.
The engine therefore gives **every car a working copy of the world** (`RollingChassis::core`), in
which that car's own bodies, joints and shapes are current. One step:

1. hand every car what it reads of the engine and the others (wind, grip, the other cars' positions
   and wakes);
2. every car runs `Car::step` on its working copy: on the calling thread or on a worker;
3. in creation order, each car's part is written into the one world (plain copies of the bodies'
   state, the joints, the shapes' collision bits; a body that was teleported tells its shapes, as
   `dBodySetPosition` does);
4. the collision pass and `dWorldStep` run once in the one world; every car hears of every contact;
5. each car gets its bodies' new state back; `Car::postStep`.

Copies do not change numbers, so the result is the game's; and since no car's step can see what
another car's step writes, the result cannot depend on threads or on the order of the tasks. That is
by construction, and it is also measured (section 3). The cars belong to the caller
(`PhysicsEngine::step(dt, time, &mut [CarSlot])`): the player's device may stay on the physics thread
(`DeviceRef::Here`, a real wheel is polled there), other cars' devices may run anywhere.

A session with **one car does not use the engine at all**: `GameSim` steps it as before. A test
(`one_car_in_the_engine_is_the_car_alone`) also shows that one car inside the engine gives the same
bits as the car alone.

Everything that makes bodies, joints or shapes must happen while the car is built
(`PhysicsEngine::build_car`: `new_in_core`, the systems, the collider mesh). Moving or tuning the car
afterwards (spawn, setup, damage, a teleport) is done on the car and reaches the world with the next
step.

## 3. Results

The oracle (`car_oracle multi`) builds the cars with the game's own `Car::Car` in one engine, gives
each its collider mesh and a scripted device, and steps the game's `PhysicsEngine::step`. Recorded
after every step, per car: every rigid body (position, quaternion, both velocities), the five damage
zones, each suspension's damage, each tyre (spin, load, slip), engine speed, gear, fuel, speed, the
collider's mask, the pit-lane switch, the two collision times, laps, the place along the line; and for
the world: the number of contact joints, a hash over everything in every contact joint (place,
normal, depth, which cars, material, `bounce_vel`), the collision clock. The port gets only each
device's reports and is compared value by value, from step 0, with no worker thread ("1 thread"),
one worker and eight workers.

### 3.1 The seven scenarios (Spa), and two more runs of two of them

| Scenario | Cars | Steps (time) | Values compared per run | Steps with contact joints | Most contact joints | Largest damage per car, km/h | No worker | 1 worker | 8 workers |
|---|---|---|---|---|---|---|---|---|---|
| `mc_side_by_side` | 2 x F2004 | 4,000 (12 s) | 1,012,000 | 937 | 24 | 0 / 0 | identical | identical | identical |
| `mc_rear_end` | 2 x F2004 | 3,000 (9 s) | 759,000 | 167 | 24 | 103 / 36 | identical | identical | identical |
| `mc_side_swipe` | 2 x F2004 | 3,333 (10 s) | 843,249 | 1,804 | 26 | 21 / 11 | identical | identical | identical |
| `mc_pile_up` | 4 x F2004 | 4,666 (14 s) | 2,337,666 | 3,389 | 45 | 67 / 64 / 66 / 94 | identical | identical | identical |
| `mc_pile_up` (the game with its own 4-thread pool) | 4 x F2004 | 4,666 (14 s) | 2,337,666 | 3,389 | 45 | 67 / 64 / 66 / 94 | identical | identical | identical |
| `mc_pit_lane` | 3 x F2004 | 11,333 (34 s) | 4,272,541 | 156 | 16 | 0 / 133 / 142 | identical | identical | identical |
| `mc_mixed` | F2004, M3 E30, Sesto Elemento, 250 GTO | 25,000 (75 s) | 12,850,000 | 12,625 | 32 | 0 / 32 / 33 / 31 | identical | identical | identical |
| `mc_field` | 6 x F2004 | 50,000 (150 s) | 37,450,000 | 47,613 | 82 | 11 / 23 / 10 / 24 / 7 / 19 | identical | identical | identical |
| `mc_rear_end` with a 488 GT3 and the multilink test car | 488 GT3, multilink GT3 (test car) | 3,000 (9 s) | 759,000 | 168 | 49 | 80 / 18 | identical | identical | identical |

What happens in them:

| Scenario | What it shows |
|---|---|
| `mc_side_by_side` | two F2004s 3.2 m apart, flat out for nine seconds (past 220 km/h), then full brakes: each drives in and out of the other's wake; the floors touch the road, the cars do not touch |
| `mc_rear_end` | an F2004 flat out into a standing one after 4.46 s: 103 km/h of damage at the rear of the one hit (and a bent suspension), 36 at the front of the other |
| `mc_side_swipe` | one F2004 turns into the one beside it: 1,800 steps of contact, damage 21 and 11 km/h |
| `mc_pile_up` | four F2004s leave the road into the barrier one after the other and into each other: damage 64 to 94 km/h, up to 45 contact joints at once |
| `mc_pit_lane` | one car parked in the pit lane, two more drive **through** it (no contact, no collision time), leave the lane nose to tail, become solid again, and later meet a barrier at 133 and 142 km/h |
| `mc_field` | six F2004s from the grid for 150 s: four finish the lap (the game counts it), with bumps on the way; contact joints in 95 % of the steps |
| `mc_mixed` | F2004, BMW M3 E30 (struts in front), Lamborghini Sesto Elemento (four-wheel drive), Ferrari 250 GTO (live rear axle) from the grid for 75 s, with contacts between them |

### 3.2 More cars, more tracks

| Run | Track | Cars | Steps (time) | Values compared per run | Steps with contact joints | Largest damage per car, km/h | Bent suspensions | No worker | 8 workers |
|---|---|---|---|---|---|---|---|---|---|
| `mc_mixed` | spa | 812 Superfast, BMW 1M, 488 GT3, Lotus 49, Abarth 500, Maserati 250F | 20,000 (60 s) | 15,760,000 | 16,309 | 38 / 58 / 63 / 59 / 19 / 50 | 2 cars | identical | identical |
| `mc_field` | spa | 12 x F2004 | 12,000 (36 s) | 17,916,000 | 9,838 | 11 / 23 / 10 / 24 / 7 / 40 / 49 / 111 / 44 / 27 / 41 / 16 | 2 cars | identical | identical |
| `mc_field` | ks_laguna_seca | 6 x F2004 | 20,000 (60 s) | 14,980,000 | 18,171 | 22 / 15 / 83 / 201 / 19 / 15 | 2 cars | identical | identical |
| `mc_mixed` | magione | F2004, M3 E30, Sesto Elemento, 250 GTO | 15,000 (45 s) | 7,710,000 | 100 | 0 / 19 / 0 / 5 | none | identical | identical |
| `mc_mixed` | spa | SF15-T, F138, Sesto Elemento, Audi R8 Plus | 12,000 (36 s) | 6,012,000 | 5,457 | 33 / 0 / 52 / 40 | none | identical | identical |
| `mc_pile_up` | spa | M3 E30, 250 GTO, Sesto Elemento, F2004 | 4,666 (14 s) | 2,398,324 | 3,215 | 49 / 46 / 53 / 66 | none | identical | identical |
| `mc_rear_end` | spa | 250 GTO, M3 E30 | 3,000 (9 s) | 798,000 | 90 | 48 / 12 | none | identical | identical |

### 3.3 Other checks

- **The game's own pool.** `car_oracle multi --scenario mc_pile_up --game-threads 4` runs the game
  with `THREADS=4` ("INITIALISING PHYSICS THREAD POOL WITH 4 THREADS"). Its recording equals the
  one made without a pool in every value except the two that hold `bounce_vel` (another run, another
  heap address), and the port matches it too.
- **The comparison can fail.** `multi-check <recording> --fault 700:1` flips one bit of car 1's forward
  speed before step 700: the first difference is reported at step 700 in `car1.body0.lvel.z`. The golden
  test flips one bit of one throttle value and must see a difference.
- **In `rustyac.exe`.** A recorded six-car drive (`--cars 6 --autodrive --record`) replayed with
  `--physics-threads 0`, `1` and `8` gives the same hash over every car's state of every step
  (`--state-hash`); likewise 12 and 24 cars with 0 and 8.
- **One car.** `cargo test --workspace` passes unchanged (every golden replay of Tasks 8 to 22b, the
  renderer's and the sound's tests, the game's replay tests). The crates of the renderer, the sound,
  the browser preview, the maths and the content were not touched, so their oracle batches were not
  run again. The one-car path of `GameSim` does not use the engine; held against `rustyac.exe` built
  from v0.23.0 (`sh tools/proofs/dumps.sh` (was `python re/scratch/task24/one_car_same.py`, deleted)):

  | Check | What | Old (v0.23.0) against new |
  |---|---|---|
  | replay `car_random` | state dump of every step, 627 MB | identical |
  | replay `car_wc_wc_stops` | state dump of every step, 106 MB | identical |
  | replay `collide_spa_wall_high` | state dump of every step, 218 MB | identical |
  | replay `collide_spa_wall_low` | state dump of every step, 153 MB | identical |
  | replay `collide_spa_lap` | state dump of every step, 657 MB | identical |
  | replay `awd_sesto_wc_spirited` | state dump of every step, 119 MB | identical |
  | replay `hy_ks_ferrari_sf15t_hy_deploy` | state dump of every step, 321 MB | identical |
  | replay `t17_bmw_m3_e30_wc_spirited` | state dump of every step, 121 MB | identical |
  | replay `t18_monza___bmw_m3_e30_trk_object` | state dump of every step, 234 MB | identical |
  | replay `conditions_spa_setup` | state dump of every step, 53 MB | identical |
  | replay `lap_trk_lap` | state dump of every step, 1661 MB | identical |
  | replay `t22b_f2004_spa_crash` | state dump of every step, 393 MB | identical |
  | screenshot: debug view, F2004 on Spa, 6 s of the line follower | car state a06ef70c21bb1d84 / a06ef70c21bb1d84, picture 396554cbe8e1a4e0 / 396554cbe8e1a4e0 | identical, same picture |
  | screenshot: AC renderer on WARP, F2004 on Spa, 6 s of the line follower | car state a06ef70c21bb1d84 / a06ef70c21bb1d84, picture e2232f5ac5f592b7 / e2232f5ac5f592b7 | identical, same picture |

## 4. How to drive with other cars

The other cars' physics is complete. They are **not drawn or heard by AC's renderer yet**; the debug
view shows them as blue boxes.

```
# five more F2004s on Spa's grid, each driven by the line follower; you drive car 0
rustyac.exe --car ks_ferrari_f2004 --track spa --spawn start --cars 6 --debug-view

# name each other car: <car>[:<skin>]:<parked | auto | file.ryin>
rustyac.exe --car ks_ferrari_f2004 --track spa --spawn start --debug-view ^
    --opponent bmw_m3_e30:auto --opponent ks_ferrari_250_gto:parked ^
    --opponent ks_ferrari_f2004:my_lap.ryin

# nobody at the wheel: six line followers, no window, 60 s, the drive written to a file
rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --spawn start --autodrive --cars 6 ^
    --headless --duration 60 --no-audio --record six.ryin

# that drive again as fast as it goes, with a hash over every car and the time of a step
rustyac.exe --replay six.ryin --headless --state-hash --physics-threads 0
rustyac.exe --replay six.ryin --headless --state-hash --physics-threads 8
```

- The other cars stand on the next slots of the spawn set: `--spawn start` the grid (`AC_START_1` ...),
  `--spawn pit` the pit boxes; with the hot-lap start (one slot) they stand on the grid. On the flat
  road (no `--track`) they stand in a row behind the player, 8 m apart, and can only be parked.
- `auto` is rustyAC's line follower (the same as `--autodrive`): it knows nothing of other cars and
  will run into them. `parked` stands on its brakes. A `.ryin` file is a drive recorded with
  `--record`: its pedals and wheel are replayed step by step, then the car brakes.
- Every other car has the automatic gearbox and clutch (a recorded one: the aids of its recording),
  its own ABS and traction control as its files have them, the session's weather and rules, no ballast.
- `--physics-threads <n>`: worker threads (0 = none). Default: the game's rule from its own
  `assetto_corsa.ini` (here: 16 logical processors, so 8 workers). It changes the speed, never the result.
- A recorded drive (`--record`) stores the other cars in its header; `--replay` builds them again.
- Checked unattended: a 6 s window (`--windowed --no-focus --duration 6 --cars 4 --debug-view`), a
  headless drive with `--opponent bmw_m3_e30:auto --opponent ks_ferrari_250_gto:parked --opponent
  ks_ferrari_f2004:<file.ryin>`, its recording and its replay (same hash with 0 and 8 workers).
  Nobody has driven among the other cars at the wheel yet.
- race.ini's `[CAR_1]` ... are **not** read yet: a session set up in Content Manager with AI cars
  still starts with your car alone (see "Not done").

## 5. Performance

This PC: Intel Core i9-9980HK (8 cores, 16 logical processors), Radeon Pro 5500M. Spa, F2004s from the grid, all driven by the line follower (which crashes them into
each other at the first corners: these are worst cases with many contacts).

### 5.1 The physics step

A recorded 20 s drive replayed as fast as it goes (`--replay --headless --state-hash`); a step has
3 ms in real time.

| Cars | No worker thread | 8 workers (the default here) | of which: the cars | the collision pass | `dWorldStep` | Same hash |
|---|---|---|---|---|---|---|
| 1 (no engine: the car alone) | 0.15 ms (5 % of a step's 3 ms) | - | - | - | - | - |
| 6 | 1.18 ms (39 %) | 1.05 ms (35 %) | 0.40 -> 0.22 ms | 0.34 ms | 0.38 ms | yes |
| 12 | 2.39 ms (80 %) | 1.76 ms (59 %) | 0.71 -> 0.22 ms | 0.55 ms | 1.02 ms | yes |
| 24 | 4.36 ms (145 %) | 2.87 ms (96 %) | 1.24 -> 0.28 ms | 0.90 ms | 2.02 ms | yes |

- Only the cars' own steps run on the pool, as in the game; the collision pass and `dWorldStep` are
  serial. With many cars in contact `dWorldStep` is the largest part (cars that touch form one island,
  and the solver's work grows faster than the number of rows).
- The game's own engine for comparison: in the oracle, six F2004s for a lap took 2.5 ms a step
  without a pool, the recording included; the port's replay of the same 50,000 steps took 2.5 ms a
  step with one thread (the comparison included) and 1.6 ms with eight workers.
- 24 cars in a pile-up are at the limit of real time here even with the pool; a field that drives
  without touching needs far less (one car alone: 0.15 ms).

### 5.2 Frames per second

`--headless --bench-render --duration 25 --camera chase`, your `video.ini`, off screen. **Only your
car is drawn** (the others are not drawn yet), so this shows what the other cars' physics takes away
from the picture, not what drawing them will cost.

| Cars in the physics | Frames per second | Physics step, average / longest | Physics kept real time |
|---|---|---|---|
| 1 | 124 | 0.17 / 5.9 ms | yes |
| 6 | 108 | 0.90 / 6.4 ms | yes |
| 12 | 80 | 2.5 / 52 ms | no: 291 of 333 steps a second |
| 24 | 33 | 7.1 / 182 ms | no (a 24-car pile-up at the first corner) |

## 6. What Task 25 (AI) needs from this

- **The slot is there.** A driver is a `ControlsProvider` (AC's `ICarControlsProvider`); the engine
  takes one per car (`CarSlot::device`). The game attaches an `AIDriver` to every car but `[CAR_0]`
  through `CarAvatar::setControlsProvider`, queued for the physics thread. In `rustyac.exe` the other
  cars' drivers are made in `sim.rs` (`opponent_driver`): an AI driver is one more arm there.
- **What an AI reads of the others** is not handed out yet: the game's `AIDriver` reads other cars'
  positions, speeds and tyre surfaces live (`stepCarsAwareness2` 0x14029f8d0, `Car::getEngagement`
  0x140270660, `getBackDistanceFromCar` 0x140270360). The engine hands each car only positions and
  wakes before the cars start. An AI needs a snapshot of more (or, to copy the game's live reads
  exactly, the no-pool order: cars before it in the list already stepped).
- **`rand()`.** The game's AI calls `rand()` inside `Car::step`; the C runtime's state is per thread,
  only the physics thread is seeded, and which worker steps which car is luck. The game's AI is
  therefore not reproducible with a pool. The port needs a rule (for example one generator per car)
  and the oracle must run the game without a pool to compare.
- **The grid and the race start** (which slot each car gets, `[SESSION_n] STARTING_POSITION`, the
  start lights' timing, the gearbox lock for all cars, AI level and aggression per `[CAR_n]`) are read
  in `re/scratch/task24/spec_4_race_ini_spawn.md` and not ported: Tasks 25 and 26.
- **GUID-0-only parts**: lap invalidation, penalties, the performance meter run for the player only in
  the game; the port already ties them to `is_first_car`.

## 7. What multiplayer needs from this

Read from the machine code (`spec_6_providers.md`):

- A remote car is **not a `Car`**. It is one rigid body in the same world (mass 1000 kg, a box
  inertia of 1.4 x 1.0 x 4.0 m, gravity on) with the car's collider mesh: category 8, mask 4 (0 for
  a spectator), sub-space `GUID + 1`. Only local car meshes meet it.
- It is moved in the step-completed phase, after `dWorldStep`: `setRotation` + `setPosition` from
  the dead-reckoned network state, and its velocity set to the network's (blended for about a second
  after the local car hit it). So during a contact it is a real body in the local car's island; what
  the solver did to it is thrown away at the end of the step.
- It is ghosted (category 0) when disconnected, when its last packet is older than 5 s, or while
  local car 0 is in the pit lane; solid again when more than 6 m away.

What the engine has for it and what is missing:

- Has: one world with sub-spaces per GUID, the collision masks, the step-completed phase, the
  per-car slot. A remote car fits as "a body the engine owns, driven by a state provider".
- Missing: a body-only participant in `PhysicsEngine` (today every participant is a whole
  `RollingChassis`), a hook that runs after `dWorldStep` in registration order, velocity setters on a
  body in `PhysicsCore`, category changes of a mesh.

**The "remote" stand-in provider (designed, not built).** A file of states (a `.mcrec` already holds
every body's position, quaternion and velocities per step, so one car of a recording can serve): the
provider owns one body with the car's mesh (category 8, mask 4), and in the step-completed phase puts
it where the file says for that step and sets its velocity, exactly as `NetCarStateProvider::step`
0x14011bd00 does without the network's dead reckoning. The local cars then collide with it as with a
remote car. It needs the three missing pieces above; the game side for the oracle would be the game's
own `NetCarStateProvider` fed with packets made from the file.

## 8. Not done

1. **Step 5: drawing and hearing every car** in `rustyac.exe` (each car's `CarAvatar` with LODs,
   lights, smoke, skid marks, damage; `CarNodeSorter`; the focused car and its keys; mirrors showing
   the right car; the smoke cap; the opponents' sound), with its `render_oracle` and `audio_oracle`
   proofs. Nothing of it was started in the renderer or the sound, so nothing is half-wired. What the
   machine code says about it is read and written down (`re/scratch/task24/spec_5_draw_hear.md`): the
   focus keys are Ctrl + numpad 3 / 1 / 2 and pick the car ahead on track, not the next index;
   `CarNodeSorter` sorts by a priority that is 10 for the car whose GUID equals the camera-mode index
   (an upstream quirk), not by distance; the mirror texture is drawn once from the focused car; the
   smoke cap applies from 26 cars and only lowers 8000 particles to 4000; engine and horn are heard
   for the 7 nearest cars, skids for 4. The debug view's blue boxes are a stand-in, not a port.
2. **race.ini's `[CAR_n]` cars in `rustyac.exe`.** They would be invisible in AC's renderer today, so
   they are not created; `--cars` / `--opponent` ask for other cars explicitly. The rules are read
   (`spec_4_race_ini_spawn.md`).
3. **The "remote" provider** (design only, section 7).
4. **Collision events to the main thread** (`Sim::stepPhysicsEvent`, the `ACClient` collision
   report): each car's events are produced (`RollingChassis::physics_events`), but only the player's
   are read by the sound, and nothing reports them anywhere else.

A note on the rules: the reader agents of step 1 wrote 15 scratch dumps into this session's temp
scratchpad (outside the repo, against rule 3); they were found and deleted. One of them read the
game's `system/cfg/assetto_corsa.ini` (reading is allowed). Nothing was written into the game's folder
or into `Documents\Assetto Corsa`.

## 9. Open questions and decisions

1. **Where the other cars start.** The game puts car n on slot n of the session's spawn set, and in a
   hot-lap session every car on the one hot-lap slot (on top of each other). rustyAC puts the other
   cars on the grid slots there. The race grid's real order (`[SESSION_n] STARTING_POSITION`: the
   player on slot F-1, the others around) is not ported: the player is on slot 0.
2. **The other cars' aids.** Until they have AI drivers they get the automatic gearbox and clutch;
   the game's AI cars have their own shifting.
3. **A black-flag teleport inside `Car::step`.** The game's other cars may see it half-done (with a
   pool) or done (without, for cars later in the list). The port's cars see the positions as they
   were at the start of the step. No scenario has a black flag with several cars.
4. **`bounce_vel` in `rustyac.exe`** is +0 (the game: a heap address, 6e-43 to 9e-43). Kept.
5. **The frame rate falls with more cars although they are not drawn** (124 FPS alone, 80 with 12
   cars' physics). The physics threads and the picture share the processor; not looked into further.
6. **`THREADS=1`** crashes the game and works here. Kept.
7. **The golden excerpt** holds this PC's Steam path of the track (`track_folder=`); another PC
   falls back to its own game folder by the track's name.
8. **Engine-level jobs.** The game drains its command queue once at the start of the step, before
   `Track::step`. The port has per-car jobs (`RollingChassis::queue`, run at the start of that car's
   step) and direct changes between steps; a job that must touch two cars at once has to be done
   between steps.
9. **From the three code reviews** (`re/scratch/task24/review_*.md`; no fault was found in the proven
   paths). Fixed: the step count is the engine's for all cars (lap times take a remainder of three from
   it; it used to restart for the other cars after "new session"); queued jobs run before anything
   else of a step, as the game's queue does; the pool can only be run by its one owner; a worker
   takes over the calling thread's rounding and denormal modes (they are equal unless a library
   changed them on one thread); the other cars no longer get the player's `assists.ini` (ABS,
   traction control: the game changes car 0 only); a recorded opponent drives with the aids of its
   recording; `--opponent` takes paths with drive letters; cars can only be built before the first
   step. Left as they are:
   - `assists.ini SLIPSTREAM` above 1 makes every car's wake longer in the game; the port sets it on
     no car (`slip_stream.speed_factor` stays 0.5), as before this task.
   - A recorded opponent starts from this session's slot, not from where its recording started.
   - `--state-hash` covers every car's rigid bodies, not its tyres or engine; `--dump-states` and
     the shared-memory pages still describe car 0 only (`numCars` is 1).
   - With `--physics-threads 1` the game's split gives the one worker no car: it is the serial time.
   - Any later code in a car's step that reads another car live, or a static, would break "the same
     bits with any number of threads" without one of the present scenarios noticing: such a read
     has to go through what the engine hands out before the cars start.

## 10. Tools

- `car_oracle multi --scenario <mc_...> --track <track> [--cars a,b,...] [--steps n] [--out dir]
  [--game-threads n] [--threads 0,1,8]`: record the game, then replay the port per thread count.
  `car_oracle multi-list` lists the scenarios; `--cars` replaces the scenario's cars (the grid
  scenarios take as many as are named).
- `car_oracle multi-check <file.mcrec> [--threads 0,1,8] [--fault <step>:<car>]`: replay only (no
  `acs.exe` needed). `multi-check <file> --steps <n> --csv-out <file.mcgold>` writes a golden excerpt.
- `python re/scratch/task24/mcrec.py <file.mcrec> [every]`: speeds, pedals, damage, the pit switch
  and the `bounce_vel` words of a recording over time.
- `cargo test --release -p rustyac-physics --test several_cars`: the golden replay (two F2004s, the
  rear-end hit, 1,900 steps, 0 and 3 workers; NOT TESTED without Assetto Corsa's folder) and the
  one-car equivalence.
- Recordings are in `oracle/multi/` (git-ignored). The four small ones were kept (`mc_rear_end`,
  `mc_side_by_side`, `mc_side_swipe`, `mc_pile_up`: 19 MB together); the others were deleted at the
  end (their `.result.txt` files stay) and are recorded again in one to two minutes each.
