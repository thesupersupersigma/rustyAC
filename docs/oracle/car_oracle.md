# Whole-car oracle: Assetto Corsa's own car on a fake flat road (Task 06)

Code: `tools/car_oracle` (new crate, outside the workspace like the other oracles),
`ac_telemetry.py` (logger v4), `check_telemetry.py` (reads converted raw recordings too).
Sources: `acs.exe` 1.16.4 (build timestamp `0x5a55e7a8`) and `acs.pdb` through the local index
(`tools/re_query.py`, `tools/disasm.py`); seven read-only research briefs written for this task
are in the git-ignored `re/scratch/car_oracle/` (`r1` … `r6`). Nothing in the game folder, the
sdk or the Ghidra project was changed, no game process was touched, and no car system was
ported. Recordings go to the git-ignored `oracle/car/` (0.93 GB for the full set; about three
minutes to regenerate).

---

## 1. Plain-English summary

A small helper program now runs Assetto Corsa's real car, not a copy of it. It loads the game's
program file into its own memory without ever starting the game, asks the game's own code to
build its physics world, an empty track and one Ferrari F2004, and then moves that world forward
333 times per simulated second, exactly as the game does. Only two things are invented: the
road, which is a perfectly flat, endless surface with full grip that answers each tyre's
question "where is the ground?", and the driver, which is a script that moves the wheel, the
pedals and the gear paddles. At every step the program writes down what the driver did, where
the body, the fuel tank and the four wheel carriers are and how fast they move, every single
push or twist that any part of the car gave them (labelled tyre, spring, damper, bump stop,
anti-roll bar, wing drag, wing lift and so on), the forces in the twenty suspension rods, about
three hundred numbers per tyre, the engine, gearbox, brakes and wings, and the telemetry packet
the game itself would have published. Twelve drives were recorded (a drop onto the road, two
standing starts through the gears, a stop from 250 km/h, three steady corners, a slalom, a
lift-off in a corner, a kerb strike and a minute of random driving), 70,673 steps in all; each
was run twice in a fresh process and the two files are identical byte for byte. To prove the
recording is honest, the tyre forces were recomputed by our own Rust tyre from the recorded tyre
inputs: all 282,692 wheel-steps came out the same in every digit, the forces and every other
tyre number too. The numbers look like an F2004: 664 kg on its wheels, 0 to 200 km/h in 5.3 s,
285 km/h on the rev limiter in sixth gear, 4.3 g of braking at 250 km/h, 2.5 g of cornering at
180 km/h. One thing is deliberately missing: on this road only the tyres can touch the ground,
not the car's floor, and when the car is first dropped its nose dips 14 mm lower than it could
in the game for 17 steps (a switch adds a real collision surface for that case). The telemetry
logger also got its "record everything, unrounded" mode.

---

## 2. How the harness builds the engine, the track and the car

### 2.1 What is real and what is fake

Real, i.e. the game's own machine code running on the game's own objects: the physics engine
object, the rigid-body library inside it (ODE 0.13.1, `dWorldStep`), the track object, the car
with all its parts (body, fuel tank, four double-wishbone suspensions, tyres, heave springs,
anti-roll bars, seven wings, engine, clutch, gearbox, differential, brakes, traction control,
automatic clutch, automatic throttle blip, steering), the game's own file readers reading the
car's data files, `PhysicsEngine::step` as the only thing called per step, the state snapshot
(`Car::getPhysicsState`) and the telemetry writer (`SharedMemoryWriter::updatePhysics`).

Fake:

| Fake | How | Why |
|---|---|---|
| The program around the code | `acs.exe` is mapped with `LoadLibraryExW(DONT_RESOLVE_DLL_REFERENCES)`; its entry point, C runtime start-up and static initialisers never run (as in `tools/tyre_oracle`) | the game must not start |
| Imports | only `msvcr120`, `msvcp120` and `kernel32` are bound. Replaced by stand-ins: `exit`, `_exit`, `abort` (report and stop), `QueryPerformanceCounter` / `Frequency`, `GetTickCount(64)`, `GetSystemTimeAsFileTime`, `winmm!timeGetTime` (a fake clock that advances 0.1 ms per query), `user32!GetAsyncKeyState` (no key is down), `user32!MessageBoxA` (prints). Every other import of the 25 DLLs (Direct3D, FMOD, Steam, sockets, shell …) points at a stub that names the import and stops | nothing may depend on the real time, keyboard, Documents folder or a device; an unexpected call is reported instead of crashing |
| Game folder | a scratch tree `re/scratch/car_oracle/root/` used as working directory: `system/cfg/assetto_corsa.ini` (section 2.3) and `content/cars/ks_ferrari_f2004/data/*` copied from `cardata/` (unpacked, no `data.acd`). No track files at all | the game's readers use relative paths |
| Track surface | the real `Track` object, but its vtable pointer is swapped for a copy in which `rayCast` (+0x08) answers from a formula (height 0, normal straight up, or a raised strip for `kerb`) and `createRayCaster` (+0x18) returns null. One hand-filled `SurfaceDef` (0xc8 bytes): `gripMod` 1.0, `isValidTrack` 1, `collisionCategory` 1, everything else 0 (no bumps, no drag, no pit lane) | "an infinite plane with one surface"; no kn5 loading |
| Racing line | none: `Track::initAISpline` runs for real and finds no files, so the line has 0 points | the car code needs the object, not its content |
| Body collider | a hand-built `Mesh` (8 corners, 12 triangles, a 0.6 × 0.2 × 1.0 m box inside the car) given to the real `Car::initColliderMesh` | `Car::updateColliderStatus` reads mesh 0 without a check; the real `collider.kn5` needs the renderer |
| Controls device | an object with a 12-entry vtable (and the RTTI pointer of the game's keyboard device in front, so the game's "is this an AI?" cast answers no) written into `Car::controlsProvider`. `acquireControls` writes steer, gas, brake, clutch, gear-up, gear-down and handbrake 0, like the keyboard device leaves the rest of `Car::controls` alone; the other entries answer like a plain device (no button pressed, force-feedback gain 1.0) | the human player's device, scripted |
| Clock | `PhysicsEngine::step(0.003, t, t)` with `t` = 60,000 ms + 3 ms per step; wall-clock imports faked as above | determinism; a session in the game never starts at time 0 (the auto-blip's "last blip" time is 0) |
| Telemetry writer | a zeroed 0x220-byte block laid out as `SharedMemoryWriter` (car, lap-invalidator and page pointers, warm-up counter 300, the four bump-stop offsets) and a zeroed 0x12a8-byte block as its `CarAvatar` (cockpit settings all 0), into whose state snapshot the real `Car::getPhysicsState` writes each step | the real writer needs the renderer-side avatar only for six values |
| Optional road mesh (`--floor`, scenario `settle_floor`) | one flat quad (two triangles, ±3 km) given to the real `Track::addSurface`, so ODE has something the body's collision boxes can rest on | off by default, see 4.4 |

### 2.2 Build sequence (function addresses are Ghidra addresses)

1. Map `acs.exe`, check the build timestamp, bind / replace imports, tell the C runtime to report
   to stderr instead of opening dialogs (`__set_app_type(1)`, `_set_abort_behavior(0, 3)`,
   `_set_error_mode(1)`, `SetErrorMode`), install a crash reporter that names the game function.
2. `srand(seed)` in the game's C runtime (the scenario's seed; the only `rand()` users on this
   path are the four suspension constructors) and reset the fake clock.
3. Write the globals of 2.4.
4. `PhysicsEngine::PhysicsEngine` 0x140262430 on 0x278 zeroed bytes from the game's allocator
   (reads `assetto_corsa.ini`, creates the real ODE core: gravity −9.806, ERP 0.3, CFM 1e-7).
   Then `physicsTime` and `gameTime` = 60,000.0, ambient 26 °C, road 30 °C (the constructor's
   own values, written again so they are fixed here).
5. `Track::Track` 0x140277100 (name `flat`, empty config) on 0x148 bytes;
   `Track::initAISpline` 0x1402782a0; optional `Track::addSurface` 0x140277e50; swap the track's
   vtable pointer. This is before the car, because `Tyre::init` asks the track for a ray caster.
6. `Car::Car` 0x14026bf00 (`ks_ferrari_f2004`, empty config) on 0x3ea0 bytes: the game loads
   every data file itself. It is car 0 (`physicsGUID` 0), the player's car.
7. `Car::initColliderMesh` 0x140273b20; write `Car::controlsProvider`; switch off the two
   bookkeeping helpers (2.4); set the automatic-clutch flags the way the game's assist option
   does.
8. Find the six rigid bodies (`Car::body`, `Car::fuelTankBody`, the hub of each suspension) and
   the 21 ODE joints (walked from the car body's joint list, named through the suspensions'
   `joints[5]` and `Car::fuelTankJoint`); give every joint a feedback buffer; install the
   recording hooks (2.5).
9. Spawn like the game does: `Car::forceRotation` 0x140270040 (it takes the direction the tail
   points: (0, 0, −1) makes the car face +z), `Car::forcePosition` 0x14026fe10 with ground point
   (0, 0, 0), `Car::setDamageLevel(0)`, `Car::resetSuspensionDamageLevel`.
10. Per step: the script chooses the controls from the car's state at the end of the previous
    step; `PhysicsEngine::step` 0x140264760 (which runs `Track::step`, wind, `Car::step`, the ODE
    step and `Car::postStep` itself); `Car::getPhysicsState` 0x140270d70;
    `SharedMemoryWriter::updatePhysics` 0x140186ef0; write the record.

One scenario runs per process: the game's objects are never destroyed (`Car::~Car` would write a
telemetry dump into the Documents folder).

### 2.3 Files given to the game

`system/cfg/assetto_corsa.ini`:

```
[PHYSICS_THREADING]
THREADS=0
[FF_EXPERIMENTAL]
ENABLE_GYRO=0
DAMPER_MIN_LEVEL=0
DAMPER_GAIN=1
```

`THREADS=0` keeps the engine from starting its thread pool (it does on machines with four or
more logical CPUs, even when the file is missing). The other keys only feed force-feedback
numbers. Deliberately absent: `cfg/race.ini` (so the dynamic-track grip stays off and no
`rand()` is drawn for it), every track file (`ai/*.ai`, `data/*.ini`, `side_l.csv` …),
`data.acd`, a `[TELEMETRY]` section.

### 2.4 Every global and object member the harness writes

| What | Where | Value | Why |
|---|---|---|---|
| `INIReader::useCache` | global 0x14151d0f9 | 0 (was 1) | its cache map is built by a start-up initialiser that never runs |
| `INIReaderDocuments::initialized` | global 0x14155a588 | 1 (was 0) | otherwise `Track::Track` asks Windows for the Documents folder and reads the user's own `cfg/race.ini`; with 1 and the empty base path it looks in the working directory, where there is none |
| timer `frequency`, `startTime` | globals 0x14155a598, 0x14155a590 | 10,000,000 and 0 | they are set by the game's start-up; with 0 the clock reads infinity |
| `isUsingQPT`, `PhysicsEngine::isTestMode` | globals 0x14151d140, 0x14155a770 | only checked (1 and 0) | |
| `PhysicsEngine::physicsTime`, `gameTime` | engine +0x20, +0xf8 | 60,000.0 | the constructor stores the wall clock; the car times its once-a-second mass refresh from it |
| `PhysicsEngine::ambientTemperature`, `roadTemperature` | engine +0x100, +0x104 | 26.0, 30.0 | fixed weather (air density 1.1856 kg/m³); wind stays at its constructor value 0 |
| `Track` vtable pointer | track +0x00 | copy with two entries replaced | the fake road |
| `Car::controlsProvider` | car +0x3d80 | the scripted device | written directly; `Car::setControllerProvider` would only add an event nobody listens to |
| `performanceMeter.isEnabled`, `telemetry.isEnabled` | car +0x3560, +0x3258 | 0 | two player-car helpers that only fill buffers (with an empty racing line the first one appends 8 bytes every step) |
| `autoClutch.useAutoOnStart` (and `useAutoOnChange` = 1 when on) | car +0x309c, +0x309d | per scenario | what `CarAvatar::setAutoClutchEnabled` does |
| vtable pointers of the 6 `RigidBodyODE` and 4 `Suspension` objects | object +0x00 | copies (with RTTI pointer) with recording entries | the hooks of 2.5 |
| `dxJoint::feedback` of the 21 joints | joint +0x68 | a 64-byte buffer each | ODE then also stores the force each joint applies; shown not to change anything else (4.2) |
| code | first 12 / 17 / 15 bytes of `Tyre::step` 0x140283800, `dWorldStep` 0x1403404c0, `dxStepIsland_Stage0_Joints` 0x140350b80 | a jump to the recorder, which calls the original through a relocated copy | the hooks of 2.5 |

Everything else is as the game's constructors and the car's files leave it: fuel 80 l, tyre
compound 0 (Slick Soft), tyre blankets on (tyres start at 80 °C), fuel / tyre wear / mechanical
damage rates 1.0, traction control present and on (slip limit 0.08, above 40 km/h), no ABS (the
car has none, so there is no ABS variant of `brake`), electronic auto-blip on, auto-shifter off,
stability aid 0, no penalties, session type 0 with start time 0 (so the session counts as
started and the jump-start rule is idle), the sleeping rule on.

### 2.5 The recording hooks

All hooks only watch: they pass the untouched arguments on to the game's own function.

| Hook | What it records |
|---|---|
| 11 entries of the rigid bodies' vtable: the seven force / torque entries (`addForceAtPos` +0x110, `addForceAtLocalPos` +0xf8, `addLocalForce` +0xd8, `addLocalForceAtPos` +0xe8, `addLocalForceAtLocalPos` +0xf0, `addTorque` +0x118, `addLocalTorque` +0xe0) and `stop` +0x70, `setVelocity`, `setAngularVelocity`, `setPosition` | one "force tape" entry per call: body, kind, the two vectors, **the return address of the caller** (a three-instruction assembly thunk reads it), and the body's force and torque accumulators right after the call |
| 6 entries of each suspension's vtable (`getHubWorldMatrix`, `getPointVelocity`, `getHubAngularVelocity`, `addForceAtPos`, `addTorque`, `addLocalForceAndTorque`) | what the tyre was told about its hub; and, for the three force entries, the caller's return address, which is attached to the body calls made inside (every hub force enters through the suspension, so this is what tells a tyre force from a spring force) |
| `Tyre::step` (entry detour) | on entry everything the tyre is about to read that other systems own; on exit the tyre's state in the layout of the single-wheel rig |
| `dWorldStep` (entry detour) | on entry: position, rotation, velocities, mass and **accumulators** of the six bodies, joint anchors / ERP / CFM, `Car::controls`; on exit: the new body state and the joint feedback |
| `dxStepIsland_Stage0_Joints` (entry detour; a local ODE function without a name in the PDB, called from `dxStepIsland` at 0x14035025f) | the accumulators once more, after ODE itself has added gravity and the gyroscopic torque (the "solver" accumulators) |

---

## 3. Recording format and converter

One file per scenario, `oracle/car/<scenario>.carrec`, little endian, every number stored as the
bits the game produced:

```
"ACCARORC"  u32 version (1)  u32 header length
header      UTF-8 text: key=value lines, then one "field <kind> <name>" line per value of the
            per-step block; kinds: f = f32, i = i32, d = f64 (two words: low, high)
steps       u32 word count (2,399, the same every step), the block's 32-bit words,
            u32 call count, then 16 words per force call
trailer     u32 0xffffffff, u32 length, text: "callsite <address> <system> <function+offset>"
            for every place in the game's code that made a call on the tape in this run
```

Header keys: `scenario`, `about`, `car`, `dt` (0.003), `clock_start_ms`, `seed`, `auto_clutch`,
`ground` (`flat` or `step x_min x_max z_from z_to height`), `floor_mesh`, `joint_forces`,
`wheels` (`lf,rf,lr,rr`), `compound`, `ray_casters` (0), `wings` (the seven names in step order),
`bodies` (`body,fuel_tank,hub_lf,hub_rf,hub_lr,hub_rr`), `joints`
(`name:ODE type:body1:body2`, type 7 = fixed, 15 = distance).

Per-step block, 2,365 named values. Units are the game's: metres, seconds, newtons, N·m, rad/s,
kg, °C; world axes x, y (up), z; the car starts at the origin facing +z and **its left side is
+x**.

| Group | Values | Content |
|---|---|---|
| `step`, `time_ms` | 2 | step number (0 …), the physics clock handed to this step |
| `script.*` | 6 | what the scripted device reported: steer (−1 … 1, positive = right), gas, brake, clutch (1 = pedal up), gearUp, gearDn |
| `controls.*` | 8 | `Car::controls` after `Car::step`, i.e. after the game's own control lock, automatic clutch and automatic blip: the controls actually applied |
| `<body>.*` for the 6 bodies | 61 each | `mass`, `inertia` (diagonal), `pre.pos`, `pre.q` (w, x, y, z), `pre.R` (3×3, row-major, world = R·local), `pre.lvel`, `pre.avel`: the state `dWorldStep` started from (= the state all car systems saw this step). `facc`, `tacc`: **the accumulators just before `dWorldStep`**. `solver.facc`, `solver.tacc`: the same after ODE added gravity and its gyroscopic torque. `tag`: the body's place in ODE's island. `post.*`: the state `dWorldStep` left |
| `joint.<name>.*` for the 21 joints | 22 each | distance joints: `anchor1` (on the car body), `anchor2` (on the hub), both in the body's own axes, `erp`, `cfm`, `distance`; the fixed joint: `qrel`, `offset`, `erp`, `cfm`; `tag` (its place in the solver's row order); and the constraint force of this step: `f1`, `t1` on body 1, `f2`, `t2` on body 2 (world axes, torque about the body's centre) |
| `tyre.<wheel>.*` | 301 each | `in_*` (60): the inputs of this `Tyre::step` in the single-wheel rig's layout (hub matrix, hub point velocity, hub angular velocity, brake torque, ABS factor, wheel speed left by the drivetrain, ground hit, car speed, temperatures …); `call0..4`, `calls`: the force calls the tyre made on its hub; `status.*` (40), contact point, normal, road axes, slide speeds, `localWheelRotation`, `thermal.*` (core, 36 patches): the tyre right after its step; `in_localWheelRotation.*`: the wheel's spin matrix on entry (the drivetrain turns it for driven wheels); `asked.*`: how often the tyre asked each question |
| `car.*`, `physics.*`, `track.*`, `world.*` | 40 | cached speed, accG, fuel, force feedback, sleeping counter, water temperature, lock / black-flag / penalty flags, damage levels, steering lock and ratio; air and road temperature, wind, step counter, clock; grip level; number of ODE bodies, joints and contact points, gravity |
| `drivetrain.*`, `engine.*` | 26 | gear, gear request, engine / gearbox / half-shaft speeds, clutch state and torque, shift cut; engine torque, limiter, traction-control override, engine life, throttle after the response curve, fuel use |
| `brakes.*`, `suspension.<wheel>.*`, `heave.*`, `arb.*` | 32 | bias, power, disc temperatures; travel, damper speed, steer torque; heave travel; bar rates |
| `aero.*`, `wing0..6.*` | 59 | air density; per wing angle of attack, CD, CL, angle, ground height, drag and lift in kg, yaw angle |
| `tc.*`, `abs.*`, `edl.*`, `stability.*`, `autoClutch.*` … | 16 | the aids' switches and outputs |
| `page.*` | 148 | the `acpmf_physics` page the game's own writer produced for this step, field by field (`SPageFilePhysics`, 592 bytes) |

Force tape, one 64-byte entry per call, in call order: body index, kind (1 `addForceAtPos`,
2 `addForceAtLocalPos`, 3 `addLocalForce`, 4 `addLocalForceAtPos`, 5 `addLocalForceAtLocalPos`,
6 `addTorque`, 7 `addLocalTorque`, 8 `stop`, 9 `setVelocity`, 10 `setAngularVelocity`,
11 `setPosition`), the call site (return address as an offset into `acs.exe`), the outer call
site (if the call was made inside a suspension force entry: where that entry was called from,
else 0), vector `a` (force or torque), vector `b` (point), and the body's force and torque
accumulators after the call. 66 entries per step when the car is moving (34 on the car body,
8 on each hub), 52 + 2 `stop` entries while it sleeps (the wings add nothing at zero speed).

Systems, by call site (`tools/car_oracle/src/sites.rs`, read from the disassembly; hub calls are
booked by their outer site):

| System | Where | What the vector is |
|---|---|---|
| `tyre` | `Tyre::addGroundContact` (vertical load: tyre spring and tyre damper summed), `Tyre::addTyreForcesV10` (grip: Fx and Fy as one world vector at the contact point; then Mz as a torque) | on the hub |
| `spring` | `Suspension::step`, first pair | spring plus packer rubber, summed; on the hub along the body's up axis, opposite on the body |
| `damper` | `Suspension::step`, second pair | damper force |
| `bumpstop` | `Suspension::step`, third and fourth pair | upper / lower bump stop (never reached in these scenarios) |
| `heave_spring`, `heave_damper`, `heave_bumpstop` | `HeaveSpring::step` | the full force on each of the two hubs and twice on the body |
| `arb` | `AntirollBar::step` | bar force on each hub and the body |
| `aero_drag`, `aero_lift` | `Wing::addDrag`, `Wing::addLift` | per wing, on the body |
| `sleep` | `Car::step` | `stop` on body and fuel tank while the car sleeps |
| `surface`, `drivetrain`, `brake`, `stability`, `steering`, `teleport`, `other`, `unknown` | | never occurred here: surface drag needs a surface with damping; the F2004's engine, drivetrain and brakes make **no** rigid-body call at all (wheel spin is a number inside tyre and drivetrain; the only reaction on a body is the tyre's grip force on the hub) |
| gravity, gyroscopic torque | inside `dWorldStep` | not on the tape: `solver.facc − facc` and `solver.tacc − tacc` |

Converter (full-precision text; f32 and f64 are printed in the shortest form that reads back to
the same bits):

- `car_oracle csv <file> --table steps` – one row per step with every field, plus derived
  columns `sum.<body>.<system>.{fx,fy,fz,tx,ty,tz}` (what each system added to each body's
  accumulators this step: the differences of the accumulator after and before each of its
  calls, summed) and `sum.<body>.gravity.f*`, `sum.<body>.gyroscopic.t*`.
- `--table tape` – one row per force call (step, sequence, body, kind, system, call site,
  function, outer site and function, the vectors, the accumulators after it).
- `--table telemetry` – the physics page per step in the column names of
  `ac_telemetry.py --to-csv`, so a recording of the real game and the oracle can be compared.
- `--from`, `--to`, `--every`, `--only prefix,prefix` limit rows and columns (the full `steps`
  table has about 3,000 columns).

---

## 4. Results

### 4.1 Per scenario

<!-- RESULTS:BEGIN -->
| Scenario | Steps | Two runs identical (hash of the file) | Tyre forces = Rust tyre | Every tyre value = Rust tyre | Tape adds up | Top speed, km/h | Long. g (min … max) | Lat. g (min … max) | Smallest floor gap after the spawn drop, mm |
|---|---|---|---|---|---|---|---|---|---|
| `settle` | 667 | yes (`9b638ff3fa0c4cd9`) | 100 % | 100 % | 100 % | 3.4 | +0.00 … +0.00 | +0.00 … +0.00 | 24.7 |
| `settle_floor` | 667 | yes (`dd271e98d3e31eff`) | 100 % | 100 % | 100 % | 3.4 | +0.00 … +0.00 | +0.00 … +0.00 | 24.7 |
| `launch_autoclutch_off` | 4667 | yes (`89ff16038b5bb2ad`) | 100 % | 100 % | 100 % | 285.2 | -0.94 … +1.63 | -0.00 … +0.00 | 5.4 |
| `launch_autoclutch_on` | 4667 | yes (`fb6817dda1e0f932`) | 100 % | 100 % | 100 % | 285.2 | -1.07 … +1.62 | -0.01 … +0.00 | 5.4 |
| `brake` | 5334 | yes (`5020b42767ee90e7`) | 100 % | 100 % | 100 % | 250.2 | -4.26 … +3.33 | -0.02 … +0.02 | 3.6 |
| `steady_corner_60` | 6667 | yes (`dac3e503f2015d1c`) | 100 % | 100 % | 100 % | 64.5 | -1.07 … +1.36 | -0.29 … +0.00 | 23.8 |
| `steady_corner_120` | 6667 | yes (`51155d2c12be1e44`) | 100 % | 100 % | 100 % | 123.8 | -1.07 … +1.48 | -1.15 … +0.00 | 19.7 |
| `steady_corner_180` | 6667 | yes (`997bae4cda1a347c`) | 100 % | 100 % | 100 % | 182.6 | -1.07 … +1.57 | -2.56 … +0.00 | 12.8 |
| `slalom` | 5334 | yes (`7a63790b761e6d3c`) | 100 % | 100 % | 100 % | 104.1 | -1.07 … +1.39 | -0.62 … +0.62 | 21.9 |
| `liftoff_oversteer` | 5334 | yes (`fa049cb40aae3885`) | 100 % | 100 % | 100 % | 182.6 | -1.07 … +1.57 | -2.72 … +0.00 | 11.0 |
| `kerb` | 4001 | yes (`cdd734cace0b755b`) | 100 % | 100 % | 100 % | 104.1 | -1.07 … +1.39 | -0.54 … +0.36 | 3.7 |
| `random` | 20001 | yes (`fb1bf0a7394e299e`) | 100 % | 100 % | 100 % | 222.3 | -2.63 … +1.63 | -1.30 … +1.29 | 10.4 |
| **all** | **70673** | | **282692 wheel-steps** | | | | | | |
<!-- RESULTS:END -->

- "Two runs identical": each scenario was run twice, each time in a new process; the value is a
  hash of every byte of the recording (header, all steps, force tape, trailer).
- "Tyre forces = Rust tyre": share of wheel-steps in which every force call the tyre made on its
  hub and its `load`, `Fx`, `Fy`, `Mz` and `feedbackTorque` are bit-identical between the game
  and `VanillaTyre` fed with the recorded inputs. "Every tyre value": the same for all 220
  recorded values per wheel-step (thermal patches, wear, flat spots, pressures …).
- "Tape adds up": share of steps in which, for every body, the accumulators after its last
  tape entry are bit-identical to the accumulators read at the entry of `dWorldStep`. In all
  282,692 wheel-steps every hub call of the tyre capture is also found on the tape, on the
  right hub, booked as `tyre`.

Scenarios (all F2004, default setup, every scenario but `settle` first waits 1.2 s):

| Scenario | Length | What the script does |
|---|---|---|
| `settle` | 2 s | nothing: the car is spawned and comes to rest |
| `settle_floor` | 2 s | the same with the collision mesh under the car (4.4) |
| `launch_autoclutch_off` | 14 s | clutch pressed, first gear, revs for 0.4 s, clutch let up over 0.6 s, flat out, up-shifts by road speed to sixth |
| `launch_autoclutch_on` | 14 s | first gear, flat out, the game's automatic clutch does the start; up-shifts to sixth |
| `brake` | 16 s | flat out to 250 km/h (reached in sixth), then full brake pedal to a stop, down-shifts on the way |
| `steady_corner_60` / `_120` / `_180` | 20 s | straight at the target speed (throttle controller), then a right-hand circle of 100 m radius (steering controller on yaw rate) |
| `slalom` | 16 s | 100 km/h, steering ±0.12 as a 0.5 Hz sine |
| `liftoff_oversteer` | 16 s | the 180 km/h circle, then the throttle is shut and the wheel held |
| `kerb` | 12 s | straight at 100 km/h; a strip 2 cm high and 20 m long under the left wheels |
| `random` | 60 s | smooth random steering, throttle and brake (seed 20040314), up- and down-shifts by road speed |

### 4.2 Determinism

All twelve scenarios are bit-identical between two runs. What was needed for that is in section
2: the fake clock, `srand`, the clock start, one process per scenario. No uninitialised memory,
thread or timer problem showed up in the recorded values.

Joint feedback was checked separately: `launch_autoclutch_off` with and without the feedback
buffers gave the same 1,961 other values in all 4,667 steps and the same force tape, so asking
ODE for the joint forces does not change the simulation (the two code paths build the same sums
in the same order).

### 4.3 Sanity

| Check | Value |
|---|---|
| Weight on the wheels at rest | 1438 + 1438 + 1813 + 1813 N = 663 kg (605 kg car + 59.2 kg fuel); ODE body 510 kg, tank 59.2 kg, hubs 2 × 19 + 2 × 28.5 kg |
| Ride height at rest (the page's `rideHeight`) | 27.6 mm front, 54.7 mm rear; centre of gravity 0.234 m above the road |
| Standing start | 0–100 km/h in 2.9 s (manual clutch; the rear wheels spin at up to nine times road speed until traction control wakes up at 40 km/h) / 2.7 s (automatic clutch); 0–200 in 5.3 / 5.1 s; 0–250 in 7.3 / 7.1 s; sixth gear after 7.2 s at 248 km/h |
| Top speed in sixth | 285 km/h at 18,800 rpm on the limiter, 9.7 s after the throttle went down; wheel loads there 3866 / 3865 / 5819 / 5818 N, i.e. about 1,310 kg of downforce; ride height 9.5 / 22.2 mm |
| Braking from 250 km/h | 4.26 g at the first moment, 4.0 g mean over the first 0.3 s, 80.3 m and 2.72 s to below 1 km/h. Without ABS the front wheels are locked from 153 km/h down and the car then slows at 2 to 3 g; the largest flat spot is 0.12 |
| Steady circle, 100 m | 60 km/h: 0.28 g, steering 0.148; 120 km/h: 1.13 g, 0.164; 180 km/h: 2.55 g, 0.233 (inner front wheel down to 740 N) |
| Lift-off at 180 km/h, 2.5 g | yaw rate rises from 0.50 to 0.57 rad/s within 0.2 s of shutting the throttle (lateral 2.7 g) and the car tightens its line without spinning |
| Kerb, 2 cm | the left front tyre's load jumps from 1,742 N to 7,405 N on the step, the left rear's to 9,183 N a moment later; the right front stays below 3,030 N |
| Engine | no damage in any scenario (life 1000 → 1000); fuel burn 0.73 l in the 14 s launch |

Two things that look odd and are the game's own behaviour:

- At the end of `brake` the car, with locked wheels, overshoots zero speed, rolls back at up to
  3.8 km/h and rocks twice before the sleeping rule freezes it. The +3.33 g in the table is that
  rebound (at 0.3 km/h).
- The spawn is not gentle: `Car::forcePosition` puts the rear wheels 1 cm and the front wheels
  9 cm above the road (it uses the larger of the two axle heights), so the nose drops 9 cm.

### 4.4 The floor (where the oracle is not the game)

In the game the car body has six collision boxes (`colliders.ini`) that rest on the track mesh.
The fake road has no mesh, so nothing stops the body. `car_oracle check` therefore computes,
from the recorded body pose, the gap between the lowest box corner and the road for every step:

- During the spawn drop (steps 52 to 68) the front floor box is below the road, at most 14.3 mm
  (15.7 mm with the clutch engaged). In the game the floor would hit the road there.
  `settle_floor` shows that variant: ODE contact joints in steps 53 to 64 (24 contact points),
  the dip stops at 9.9 mm, and the car ends at the same height and within 1 N of the same
  wheel loads.
- After the spawn drop the gap stays positive in every scenario; the smallest are 3.6 mm
  (`brake`, nose dive), 3.7 mm (`kerb`) and 5.4 mm (285 km/h). So from step 69 on the
  recordings are what the game would do from that state on a flat road.

The default is contact-free on purpose: the next Rust stages have no collision code, and a
recording without contacts can be replayed by a solver that only knows the suspension rods.
`--floor` adds the mesh to any scenario.

### 4.5 Things the runs settled

| Earlier open question | Answer from the oracle |
|---|---|
| Does a whole AC car run outside the game? (`car.md` 5.2 no. 2) | yes; `Car::Car` and the first step worked on the first try with the recipe of `body.md` 9.3 |
| Island and row order inside `dWorldStep` (no. 3) | bodies: hub RR, car body, fuel tank, hub LF, hub RF, hub LR (`tag` 0 … 5); joint blocks: fuel-tank joint, then the five rods of LF, RF, LR, RR in creation order (`tag` 0 … 20). Constant in all runs |
| Timing of the telemetry page (no. 5) | by the unpatched code: `velocity`, the angles, `localAngularVel` and `localVelocity` are the body **after** this step, `speedKmh` and `accG` are from **before** it. Example, step 1500 of the launch: `speedKmh / 3.6` = 32.14463 = cached speed, `|velocity|` = 32.18044 = the new body speed. The recorded Spa lap does not show this offset, so it came from the patched game |
| Sign conventions (no. 11) | car axes: +x left, +y up, +z forward. Positive steering input turns right. In a right turn the yaw rate (`localAngularVel.y`) and the lateral `accG.x` are negative |
| Does the engine or the brake twist a body? | no: on this car no call on a rigid body comes from the drivetrain, the engine or the brakes |
| Is asking ODE for joint forces harmless? | yes (4.2) |
| Tyre in the car versus tyre on the rig | identical once two things are treated as inputs: the wheel speed and the wheel's spin matrix, both rewritten by the drivetrain for the driven wheels between tyre steps |

---

## 5. What did not work, and the crash causes that were hit

None of the crashes predicted in `car.md` 4.2 happened, because the recipe there was followed
(mesh collider, controls device, INI cache off, no `data.acd`); the empty racing line did not
crash. What went wrong on the way, and the fix:

| Problem | Cause | Fix |
|---|---|---|
| The first version read the user's real `Documents/Assetto Corsa/cfg/race.ini` | `shell32` had been bound "to be safe", so `INIReaderDocuments` found the real Documents folder | `shell32` is not bound; `INIReaderDocuments::initialized` = 1 |
| The car drove backwards on throttle | the fake device zeroed the whole `Car::controls`; `requestedGearIndex` 0 means "H-shifter in reverse" | write only what a keyboard device writes; the constructor's −1 stays |
| The car faced −z | `Car::forceRotation` takes the direction of the tail | pass (0, 0, −1) |
| Body mass would never be refreshed | the engine constructor stores the wall clock (infinite here) in `physicsTime`, and the car times its mass refresh from it | set `physicsTime` after the constructor (found by reading, before it could bite) |
| Scripts shifted 2 → 3 → 4 → 5 at 40 km/h | during a shift the gear reads "neutral", which the script took for "not started yet"; and wheelspin at the start reached shift revs | a "started" latch; shift by the revs the road speed implies |
| Steering controller ran to its stop | a right turn is a negative yaw rate | sign |
| Rust tyre differed for the rear wheels (`localWheelRotation` only) | the drivetrain turns the driven wheels' spin matrix after the tyre step | recorded on entry and fed to the Rust tyre as an input |
| Floor boxes below the road at the spawn and on a 3 cm kerb | no collision mesh (4.4) | reported per step by `check`; kerb lowered to 2 cm; `--floor` option |
| Quaternion columns were labelled x, y, z, w | ODE's order is w, x, y, z | named explicitly |

Not done:

- The tyres' ground rays go through the track's `rayCast` entry (the path the game's own tyre
  test bench uses), not through a per-wheel ODE ray against a mesh. In `Tyre::step` both paths
  fill the same hit point, normal and surface and join at 0x140283b2d, so the tyre maths is the
  same; what differs is who finds the hit. The fake answers like the game's 3 m ray: a hit when
  the road is at most 3 m below the ray's start (2 m above the wheel centre).
- Only double-wishbone cars: the harness stops on any other suspension type (the hub pointer
  and joint list are read from the `Suspension` layout).
- The graphics and static telemetry pages are not produced (they need the renderer-side
  objects); only the physics page is.
- Bump stops, surface drag, ABS, EDL, DRS, KERS and collisions never act in these twelve runs,
  so there is no recorded example of them.

---

## 6. Commands

```
cd tools/car_oracle
cargo build --release

car_oracle list                                   # the scenarios
car_oracle all                                    # every scenario twice + all checks, writes oracle/car/results.md
car_oracle run --scenario brake                   # one scenario -> oracle/car/brake.carrec, prints its hash
car_oracle run --scenario brake --floor           # with the collision mesh -> brake_floor.carrec
car_oracle run --scenario random --steps 3000 --hash-only
car_oracle check oracle/car/brake.carrec          # tyre replay, tape checks, headline numbers, floor gap
car_oracle diff a.carrec b.carrec [--ignore .f1.,.t1.,.f2.,.t2.]
car_oracle csv oracle/car/brake.carrec --table steps --only step,controls.,body.,sum.body. --every 10
car_oracle csv oracle/car/brake.carrec --table tape --from 3000 --to 3010
car_oracle csv oracle/car/brake.carrec --table telemetry
```

(`car_oracle` = `target/release/car_oracle.exe`. Common options: `--acs <path to acs.exe>`,
`--out <dir>`, `--root <scratch game folder>`, `--verbose` to see the game's own console
output.) `python tools/car_oracle/gen_functions.py` regenerates the embedded function table from
`re/index` (only needed if the index changes).

Telemetry logger v4:

```
python ac_telemetry.py --laps 3 --print --out lap.csv            # as before: rounded CSV, same columns as v3
python ac_telemetry.py --raw --laps 3 --print --out lap.acraw    # whole physics + graphics page per packet, static page once
python ac_telemetry.py --to-csv lap.acraw [--out lap.csv]        # full-precision CSV (+ lap.static.csv); no game needed
```

The raw file is: `"ACTLMRAW"`, version 4, header size, the three page sizes (592, 304, 684),
record size, start time, the static page; then per packet an f64 time and the two pages. It
stays read-only on the shared memory, and now reads each page twice and keeps it only when both
reads agree (the game copies the page without a lock). It was tested without the game by a
script that plays the game's part (`re/scratch/car_oracle/test_logger_v4.py`): every recorded
page byte-identical to a page that was written, every converted float reading back to the same
bits.

---

## 7. Open questions

### 7.1 Choices made while working unattended

1. **Contact-free by default** (4.4). The alternative, the collision mesh in every scenario,
   is one flag away; say if the goldens should be made that way.
2. **Start state**: the game's own spawn (with its 9 cm nose drop) followed by 1.2 s of rest,
   not a hand-made rest pose.
3. **Weather and session**: 26 °C air, 30 °C road, no wind, no session, damage and wear rates
   at the game's 100 %, default fuel (80 l), default compound, blankets on. Auto-blip and
   traction control stay as the car's files set them (both on). "Auto-clutch off" clears only
   the start aid; the clutch dip on down-shifts stays, as it does in the game for this car
   (`USE_ON_CHANGES=1`).
4. **The physics clock starts at 60 s**, not at 0.
5. **Two helper objects of the player car are switched off** (`PerformanceMeter`, `Telemetry`).
   They do not touch the car's motion.
6. **Joint forces are on in the goldens** (ODE feedback), after showing that it changes nothing.
7. **Scripts are closed-loop** (speed holding, yaw-rate steering, shifting by road speed). They
   only read the car's state of the previous step, so the runs are still deterministic, but a
   Rust car that differs slightly will be driven slightly differently. For a strict
   comparison replay the recorded `script.*` columns instead.
8. **`kerb` uses a 2 cm strip** (3 cm made the floor touch it).
9. **Recordings are big** (13.8 kB per step) and stay outside git; nothing compact is checked
   in yet. A hash list is in the table above.
10. **Research briefs**: seven read-only subagents (three at a time) wrote hand-over notes from
    the disassembly; six had arrived when this was written and all were used; the seventh (the
    tyre inside the car) was overtaken by the measurement itself.
11. **Logger v4**: `--raw` is a switch that changes the output format (default extension
    `.acraw`); the converted CSV starts with the v3 columns so `check_telemetry.py` reads it.

### 7.2 Questions

1. Are the recorded hashes the same on another PC? The game code is the same, but `sinf`,
   `cosf`, `powf` and friends come from the installed `msvcr120.dll`, which may pick a different
   code path on another CPU.
2. Should the first Rust rigid-body test replay from step 0 (through the spawn drop) or from a
   later step? Both are possible: every step records the full state before and after.
3. The gyroscopic torque that ODE adds inside the step is now recorded as a difference
   (`solver.tacc − tacc`), not derived. Its formula still has to be read from the code for the
   port (`physics_engine.md` describes it from the ODE source).
4. A lap from the unpatched game recorded with `ac_telemetry.py --raw` could now be laid next
   to the oracle's `telemetry` table (same columns) to see the one-step offset of 4.5 in the
   real game.
5. Other cars (strut, axle, multilink suspensions; turbo, KERS, ABS) need small additions to
   the harness (hub and joint discovery per type) before they can be recorded.
