# Whole-car oracle: Assetto Corsa's own car on a fake flat road (Task 06)

Code: `tools/car_oracle` (new crate, outside the workspace like the other oracles),
`ac_telemetry.py` (logger v4), `check_telemetry.py` (reads converted raw recordings too).
Sources: `acs.exe` 1.16.4 (build timestamp `0x5a55e7a8`) and `acs.pdb` through the local index
(`tools/re_query.py`, `tools/disasm.py`). Six read-only research briefs (`r1` … `r6`) and three
independent reviews of the harness, of this report and of the oracle's fidelity to the game are
in the git-ignored `re/scratch/car_oracle/`. Nothing in the game folder, the sdk or the Ghidra
project was changed, no game process was touched, and no car system was ported. Recordings go
to the git-ignored `oracle/car/` (0.97 GB for the full set; about three minutes to regenerate).

Later additions to the harness are described in the reports of the tasks that made them, not
here: the `pt_*` scenarios with handbrake, H-shifter, brake-bias clicks and the automatic
gearbox (Task 09, `docs/port/drivetrain.md`); the `wc_*` scenarios with the DRS and headlight
buttons, jobs called through the game's own functions (control locks, gentle stop, penalties),
a pit-lane road, the stability aid, wind through the game's `setWind`, damage levels, and more
recorded values of wings, aids and car-level state (Task 10, `docs/port/whole_car.md` 3.3).
The scenarios of this report still record the same step data, byte for byte (checked by
recording `settle` and `kerb` again after Task 10); only the text header is one line longer
(`auto_shifter=0`, since Task 09).

---

## 1. Plain-English summary

A small helper program now runs Assetto Corsa's real car instead of a copy of it: it loads the
game's program file into its own memory without ever starting the game, asks the game's own
code to build its physics world, an empty track and one Ferrari F2004, and then moves that
world forward 333 times per simulated second, exactly as the game does. The surroundings are
stand-ins: the road is a perfectly flat, endless surface with full grip that answers each
tyre's question "where is the ground?", the driver is a script that moves the wheel, the pedals
and the gear paddles, the clock is a counter, and the car is prepared the way the game prepares
it when a session starts (medium tyres, and setup values rounded the way the game's setup
screen rounds them, which was checked line by line against a real game log for another car).
At every step the program writes down what the driver did, where the body, the fuel tank and
the four wheel carriers are and how fast they move, every single push or twist that any part
of the car gave them (labelled tyre, spring, damper, anti-roll bar, wing drag, wing lift and so
on), the forces in the twenty suspension rods, about three hundred numbers per tyre, the
engine, gearbox, brakes and wings, and the telemetry packet the game itself would have
published. Twelve drives were recorded (a drop onto the road, the same drop with a solid floor
under the car, two standing starts through the gears, a stop from 250 km/h, three steady
corners, a slalom, a lift-off in a corner, a kerb strike and a minute of random driving),
70,673 steps in all; each was run twice from scratch and both runs give the same checksum over
every byte of the recording. To prove the recording is honest, the tyre forces were recomputed
by our own Rust tyre from the recorded tyre inputs, and all 282,692 wheel-steps came out the
same in every digit; in addition each of the 4.59 million recorded pushes was shown to add up,
digit for digit, to the totals the game then used to move the car. The numbers look like an F2004:
664 kg with fuel, 0 to 200 km/h in 5.3 s, 285 km/h on the rev limiter in sixth gear, 4.25 g of
braking at 250 km/h, 2.5 g of cornering at 180 km/h. One thing differs from the game on
purpose: on this road only the tyres can touch the ground, not the car's floor; in these
drives that never matters, because the game itself ignores body contact for the first 0.75 s
after a session starts (when the nose briefly dips) and afterwards the floor always stays at
least 3.5 mm above the road. The telemetry logger also got its "record everything, unrounded"
mode with a converter to full-precision tables.

---

## 2. How the harness builds the engine, the track and the car

### 2.1 What is real and what is fake

Real, i.e. the game's own machine code running on the game's own objects: the physics engine
object, the rigid-body library inside it (ODE 0.13.1, `dWorldStep`), the track object, the car
with all its parts (body, fuel tank, four double-wishbone suspensions, tyres, heave springs,
anti-roll bars, seven wings, engine, clutch, gearbox, differential, brakes, traction control,
automatic clutch, automatic throttle blip, steering, setup manager), the game's own file
readers reading the car's data files, `PhysicsEngine::step` as the only thing called per step,
the state snapshot (`Car::getPhysicsState`) and the telemetry writer
(`SharedMemoryWriter::updatePhysics`).

Fake:

| Fake | How | Why |
|---|---|---|
| The program around the code | `acs.exe` is mapped with `LoadLibraryExW(DONT_RESOLVE_DLL_REFERENCES)`; its entry point, C runtime start-up and static initialisers never run (as in `tools/tyre_oracle`) | the game must not start |
| Imports | only `msvcr120`, `msvcp120` and `kernel32` are bound. Replaced by stand-ins: `exit`, `_exit`, `abort` (report and stop), `QueryPerformanceCounter` / `Frequency`, `GetTickCount(64)`, `GetSystemTimeAsFileTime`, `winmm!timeGetTime` (a fake clock that advances 0.1 ms per query), `user32!GetAsyncKeyState` (no key is down), `user32!MessageBoxA` (prints). Every other import of the 25 DLLs (Direct3D, FMOD, Steam, sockets, shell …) points at a stub that names the import and stops | nothing may depend on the real time, keyboard, Documents folder or a device; an unexpected call is reported instead of crashing |
| The game's console output | the game's `stdout` (it prints every key it loads) is reopened on `NUL` with its own `freopen`; with `--verbose` it is left visible and unbuffered (`setvbuf`) | quiet runs; the game's own "Setup change" lines when wanted |
| Game folder | a scratch tree `re/scratch/car_oracle/root/` used as working directory: `system/cfg/assetto_corsa.ini` (section 2.3) and `content/cars/ks_ferrari_f2004/data/*` copied from `cardata/` (unpacked, no `data.acd`). `content/tracks` and `system/cfg/tyre_smoke.ini` are deleted from the scratch tree before every run, so no track file exists | the game's readers use relative paths |
| Track surface | the real `Track` object, but its vtable pointer is swapped for a copy in which `rayCast` (+0x08) answers from a formula (height 0, normal straight up, or a raised strip for `kerb`) and `createRayCaster` (+0x18) returns null. One hand-filled `SurfaceDef` (0xc8 bytes): `gripMod` 1.0, `isValidTrack` 1, `collisionCategory` 1, everything else 0 (no bumps, no drag, no pit lane) | "an infinite plane with one surface"; no kn5 loading |
| Racing line | none: `Track::initAISpline` runs for real and finds no files, so the line has 0 points | the car code needs the object, not its content |
| Body collider | a hand-built `Mesh` (8 corners, 12 triangles, a 0.6 × 0.2 × 1.0 m box inside the car) given to the real `Car::initColliderMesh`, which also takes the car's bounding box from it | `Car::updateColliderStatus` reads mesh 0 without a check; the real `collider.kn5` needs the renderer. The six collision boxes of `colliders.ini` are the real ones |
| Controls device | an object with a 12-entry vtable (and the RTTI pointer of the game's keyboard device in front, so the game's "is this an AI?" cast answers no) written into `Car::controlsProvider`. `acquireControls` writes steer, gas, brake, clutch, gear-up, gear-down and handbrake 0; like the keyboard device it leaves the rest of `Car::controls` alone. The other entries answer like a plain device (no button pressed, force-feedback gain 1.0) | the human player's device, scripted |
| Session start | what the game's setup screen does to the player's car when a session starts with the default setup, redone by the harness (section 2.6): the default tyre compound and the round trip of every setup value through the screen's whole-number spinners. The values are applied by the game's own `Tyre::setCompound` and `SetupManager::step` | without it the car is not the car the game drives (found by review, section 5) |
| Clock | `PhysicsEngine::step(0.003, t, t)` with `t` = 60,000 ms + 3 ms × (step + 1); wall-clock imports faked as above | determinism; a session in the game never starts at time 0 (the auto-blip's "last blip" time is 0) |
| Telemetry writer | a zeroed 0x220-byte block laid out as `SharedMemoryWriter` (car, lap-invalidator and page pointers, warm-up counter 300, the car's eight bump-stop limits) and a zeroed 0x12a8-byte block as its `CarAvatar` (cockpit settings all 0), into whose state snapshot the real `Car::getPhysicsState` writes each step | the real writer needs the renderer-side avatar only for a handful of values |
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
   own temperatures, written again so they are fixed here).
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
   recording hooks (2.5); wire the telemetry writer block.
9. Spawn like the game does: `Car::forceRotation` 0x140270040 (it takes the direction the tail
   points: (0, 0, −1) makes the car face +z), `Car::forcePosition` 0x14026fe10 with ground point
   (0, 0, 0), `Car::setDamageLevel(0)` 0x140275b20, `Car::resetSuspensionDamageLevel`
   0x140275970.
10. Session start (2.6): `Tyre::setCompound` 0x1402834e0 with the car's default compound on the
    four tyres, then the setup-screen round trip written into the car's setup items.
11. Per step: the script chooses the controls from the car's state at the end of the previous
    step; `PhysicsEngine::step` 0x140264760 (which runs `Track::step`, wind, `Car::step`, the ODE
    step and `Car::postStep` itself; in step 0 the car's `SetupManager::step` 0x14028d090 applies
    the setup values of step 10); `Car::getPhysicsState` 0x140270d70;
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
[LOW_SPEED_FF]
SPEED_KMH=3
MIN_VALUE=0.01
```

`THREADS=0` keeps the engine from starting its thread pool (it does on machines with four or
more logical CPUs, even when the file is missing). The other keys only feed force-feedback
numbers and have the values of the game's own file. Deliberately absent: `cfg/race.ini` (so the
dynamic-track grip stays off and no `rand()` is drawn for it), every track file (`ai/*.ai`,
`data/*.ini`, `side_l.csv` …), `data.acd`, a `[TELEMETRY]` section. A hash of the car's data
files (names and contents) is stored in every recording's header (`car_data_hash`).

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
| `SetupItem::newValue`, `SetupItem::attached` of every setup item that has a section in `setup.ini` | items of `Car::setupManager` (car +0x35d0), item +0x54, +0x58 | the rounded value, 1 | the setup-screen round trip (2.6); the game's `SetupManager::step` then writes the car's values itself |
| vtable pointers of the 6 `RigidBodyODE` and 4 `Suspension` objects | object +0x00 | copies (with RTTI pointer) with recording entries | the hooks of 2.5 |
| `dxJoint::feedback` of the 21 joints | joint +0x68 | a 64-byte buffer each | ODE then also stores the force each joint applies; shown not to change anything else (4.2) |
| code | first 12 / 17 / 15 bytes of `Tyre::step` 0x140283800, `dWorldStep` 0x1403404c0, `dxStepIsland_Stage0_Joints` 0x140350b80 | a jump to the recorder, which calls the original through a relocated copy | the hooks of 2.5 |

Everything else is as the game's constructors and the car's files leave it: fuel 80 l, tyre
blankets on (tyres start at 80 °C), fuel / tyre wear / mechanical damage rates 1.0, traction
control present and on (slip limit 0.08, above 40 km/h), no ABS (the car has none, so there is
no ABS variant of `brake`), electronic auto-blip on, auto-shifter off, stability aid 0, no
penalties, session type 0 with start time 0 (so the session counts as started and the
jump-start rule is idle), the sleeping rule on. The game's own rule that the car's joints run
with ERP 0.9 instead of 0.3 below 1 m/s is visible in the recorded joint values (`erp`).

Not done although the game does it at a session start: `setNoCollisionSteps(250)` (the engine
skips collision detection for the first 250 steps). Without a road mesh there is nothing to
collide with, so it changes nothing in the default runs; see 4.4 for what it means.

### 2.5 The recording hooks

All hooks only watch: they pass the untouched arguments on to the game's own function.

| Hook | What it records |
|---|---|
| 12 entries of the rigid bodies' vtable: the seven force / torque entries (`addForceAtPos` +0x110, `addForceAtLocalPos` +0xf8, `addLocalForce` +0xd8, `addLocalForceAtPos` +0xe8, `addLocalForceAtLocalPos` +0xf0, `addTorque` +0x118, `addLocalTorque` +0xe0) and `stop` +0x70, `setVelocity` +0x80, `setAngularVelocity` +0x88, `setPosition` +0x90, `setRotation` +0x98 | one "force tape" entry per call: body, kind, the two vectors, **the return address of the caller** (a three-instruction assembly thunk reads it), and the body's force and torque accumulators right after the call |
| 6 entries of each suspension's vtable (`getHubWorldMatrix`, `getPointVelocity`, `getHubAngularVelocity`, `addForceAtPos`, `addTorque`, `addLocalForceAndTorque`) | what the tyre was told about its hub; and, for the three force entries, the caller's return address, which is attached to the body calls made inside (every hub force enters through the suspension, so this is what tells a tyre force from a spring force) |
| `Tyre::step` (entry detour) | on entry everything the tyre is about to read that other systems own; on exit the tyre's state in the layout of the single-wheel rig |
| `dWorldStep` (entry detour) | on entry: position, rotation, velocities, mass and **accumulators** of the six bodies, joint anchors / ERP / CFM, `Car::controls`; on exit: the new body state and the joint feedback |
| `dxStepIsland_Stage0_Joints` (entry detour; a local ODE function without a name in the PDB, called from `dxStepIsland` at 0x14035025f) | the accumulators once more, after ODE itself has added gravity and the gyroscopic torque (the "solver" accumulators) |

The harness stops with a message if `dWorldStep` does not run exactly once per step, if ODE
steps more than one island, or if the controls device is not asked exactly once.

### 2.6 The session start

In the game, entering a session with the default setup changes the car in two ways before the
first step, both done by the setup screen (user-interface code that cannot run here). The
harness redoes them and lets the game's own physics code apply them:

1. **Tyre compound.** The screen calls `Tyre::setCompound` with `tyres.ini [COMPOUND_DEFAULT]
   INDEX`. For the F2004 that is index 1, Slick Medium; the constructor alone leaves index 0,
   Slick Soft. The harness calls the game's `Tyre::setCompound(1)` on the four tyres; the index
   is in the header (`compound`).
2. **Setup round trip.** Every `SetupManager` item that has a section in the car's `setup.ini`
   is shown as a spinner holding a whole number; on entry the screen sets each spinner from the
   car's value and writes the spinner's value back. A value that is not on the spinner's grid
   comes back changed. The harness mirrors `SetupScreen::loadINI` 0x14017d950,
   `SetupTab::addItem` 0x140183850 and the spinner job 0x140183620 (three display modes, C-style
   truncation, clamping to the spinner's range), writes the result into the items, and the
   game's `SetupManager::step` applies it in step 0.

For the F2004, 11 values change (header key `setup_changes`):

| Item | Car file value | After the round trip |
|---|---|---|
| `DIFF_POWER`, `DIFF_COAST` | 0.2, 0.3 | 0.19999999, 0.29999998 |
| `DIFF_PRELOAD` | 20 | 21 |
| `CAMBER_LF` / `RF` | ±0.052359 rad (3.0°) | ±0.050614547 rad (2.9°) |
| `CAMBER_LR` / `RR` | ±0.034906 rad (2.0°) | ±0.033161256 rad (1.9°) |
| `TOE_OUT_LF` / `RF` | 0.00012 | 0.0001 |
| `PACKER_RANGE_LR` / `RR` | 0.06 m | 0.059000004 m |

Check against the real game: `car_oracle setup-check --car bmw_z4_gt3` builds that car, does the
same round trip and lets the game's own code print what it changes. The 24 "Setup change for
Car" lines it prints are identical, name by name and digit by digit, to the 24 lines a real
session with that car left in the game's `log.txt` (read only). There is no real F2004 session
in that log, so the F2004's 11 lines rest on the same code, not on a direct comparison.

---

## 3. Recording format and converter

One file per scenario, `oracle/car/<scenario>.carrec`, little endian, every number stored as the
bits the game produced:

```
"ACCARORC"  u32 version (1)  u32 header length
header      UTF-8 text: key=value lines, then one "field <kind> <name>" line per value of the
            per-step block; kinds: f = f32, i = i32, x = u32 shown in hex,
            d = f64 (two words: low, high)
steps       u32 word count (2,402, the same every step), the block's 32-bit words,
            u32 call count, then 16 words per force call
trailer     u32 0xffffffff, u32 length, text: "callsite <address> <system> <function+offset>"
            for every place in the game's code that made a call on the tape in this run
```

Header keys: `scenario`, `about`, `car`, `car_data_hash`, `dt` (0.003), `clock_start_ms`,
`seed`, `auto_clutch`, `ground` (`flat` or `step x_min x_max z_from z_to height`),
`floor_mesh`, `joint_forces`, `wheels` (`lf,rf,lr,rr`), `compound`, `setup_changes`
(`name:old->new`, …), `ray_casters` (0), `wings` (the seven names in step order: `BODY`,
`FRONT`, `REAR`, `DIFFUSER_F`, `DIFFUSER_RL`, `DIFFUSER_RR`, `FIN`), `bodies`
(`body,fuel_tank,hub_lf,hub_rf,hub_lr,hub_rr`), `joints` (`name:ODE type:body1:body2`,
type 7 = fixed, 15 = distance).

Per-step block, 2,368 named values in 2,402 words. Units are the game's: metres, seconds,
newtons, N·m, rad/s, kg, °C; world axes x, y (up), z; the car starts at the origin facing +z
and **its left side is +x**.

| Group | Values | Content |
|---|---|---|
| `step`, `time_ms` | 2 | step number (0 …), the physics clock handed to this step (60,003 for step 0) |
| `script.*` | 6 | what the scripted device reported: steer (−1 … 1, positive = right), gas, brake, clutch (1 = pedal up), gearUp, gearDn |
| `controls.*` | 8 | `Car::controls` at the entry of `dWorldStep`, i.e. after the game's own control lock, automatic clutch and automatic blip: the controls actually applied (steer, gas, brake, clutch, handBrake, gearUp, gearDn, requestedGearIndex) |
| `<body>.*` for the 6 bodies | 61 each | `mass`, `inertia` (diagonal), `pre.pos`, `pre.q` (w, x, y, z), `pre.R` (3×3, row-major, world = R·local), `pre.lvel`, `pre.avel`: the state `dWorldStep` started from. `facc`, `tacc`: **the accumulators just before `dWorldStep`**. `solver.facc`, `solver.tacc`: the same after ODE added gravity and its gyroscopic torque. `tag`: the body's place in ODE's island. `post.*`: the state `dWorldStep` left |
| `joint.<name>.*` for the 21 joints | 22 each | distance joints: `anchor1` (on the car body), `anchor2` (on the hub), each in its body's own axes, `erp`, `cfm`, `distance`; the fixed joint: `qrel`, `offset`, `erp`, `cfm`; `tag` (its place in the solver's row order); and the constraint force of this step: `f1`, `t1` on body 1, `f2`, `t2` on body 2 (world axes, torque about the body's centre) |
| `tyre.<wheel>.*` | 301 each | `in_*` (60): the inputs of this `Tyre::step` in the single-wheel rig's layout (hub matrix, hub point velocity, hub angular velocity, brake torque, ABS factor, wheel speed left by the drivetrain, ground hit, car speed, temperatures …); `call0..4`, `calls` (56): the force calls the tyre made on its hub; `status.*` (40), contact point, normal, road axes, slide speeds, `localWheelRotation`, `thermal.*` (77: core and the 36 surface patches): the tyre right after its step; `in_localWheelRotation.*` (16): the wheel's spin matrix on entry (the drivetrain turns it for driven wheels); `asked.*` (4): how often the tyre asked each question; `ambiguous`: 1 if the hub gave two different answers to one question in this step (never happened) |
| `car.*`, `physics.*`, `track.*`, `world.*` | 36 | steering signal, speed, accG, fuel, mass, force feedback, sleeping counter, water temperature, lock / black-flag / penalty values, damage levels, ballast, steering lock and ratio; air and road temperature, wind, step counter, clock; grip level; number of ODE bodies, joints and contact points, gravity |
| `drivetrain.*`, `engine.*` | 29 | gear, engine / gearbox / half-shaft speeds, clutch state and torque, ratio, shift cut, differential ramps and preload, gear request; engine torque, limiter, traction-control override, engine life, throttle after the response curve, fuel use |
| `brakes.*`, `suspension.<wheel>.*`, `heave.*`, `arb.*` | 32 | bias, power, disc temperatures; travel, damper speed, steer torque and angle, lower bump stop; heave travel; bar rates |
| `aero.*`, `wing0..6.*` | 59 | air density, the car's dynamic CD / CL; per wing angle of attack, CD, CL, angle, ground height, drag and lift in kg, yaw angle |
| `tc.*`, `abs.*`, `edl.*`, `stability.*`, `autoClutch.*` … | 16 | the aids' switches and outputs |
| `page.*` | 148 | the `acpmf_physics` page the game's own writer produced for this step, field by field (`SPageFilePhysics`, 592 bytes) |

Things to know when reading the values:

- `pre.*` is read at the entry of `dWorldStep`. On steps in which the car is frozen by the
  sleeping rule, `Car::step` has by then called `stop` on the body and the fuel tank, so their
  `pre` velocities are zero.
- `car.speed` and `car.accG` are the car's own cached values, computed during `Car::step` from
  the state before this step's ODE step.
- `page.*`: `packetId` is the step number. `velocity`, the angles, `localVelocity` and
  `localAngularVel` are the body after this step, `speedKmh` and `accG` are from before it
  (4.5). `rideHeight` and `currentMaxRpm` come from the state snapshot, which the harness
  refreshes every step; in the game it is refreshed once per drawn frame, so there these two
  can be up to a frame old. `performanceMeter` is meaningless (the helper is off) and the
  cockpit values (engine-brake and ERS settings) are 0. Only the physics page exists; the
  graphics and static pages need renderer-side objects.

Force tape, one 64-byte entry per call, in call order: body index, kind (1 `addForceAtPos`,
2 `addForceAtLocalPos`, 3 `addLocalForce`, 4 `addLocalForceAtPos`, 5 `addLocalForceAtLocalPos`,
6 `addTorque`, 7 `addLocalTorque`, 8 `stop`, 9 `setVelocity`, 10 `setAngularVelocity`,
11 `setPosition`, 12 `setRotation`), the call site (return address as an offset into
`acs.exe`), the outer call site (if the call was made inside a suspension force entry: where
that entry was called from, else 0), vector `a` (force or torque), vector `b` (point), and the
body's force and torque accumulators after the call. A normal step of the moving car has 66
entries (34 on the car body, 8 on each hub). There are fewer when something is skipped: 54
while the car is frozen (the seven wings add nothing then, and two `stop` entries appear),
one less per tyre that is off the ground (no load call), 40 in step 0, and two less per spring
whose travel is not positive. That last case is not only the spawn drop: in the 180 km/h circle
the inner front spring is fully extended (travel −0.8 mm), so `steady_corner_180` has 64
entries per step from step 3352 to the end (3,315 steps) and `liftoff_oversteer` in 1,129
steps between 3352 and 4525.

Systems, by call site (`tools/car_oracle/src/sites.rs`, read from the disassembly; hub calls are
booked by their outer site, so the trailer's two `Suspension::addForceAtPos` /
`Suspension::addTorque` lines, labelled `other`, are only the inner half of such calls):

| System | Where | What the vector is |
|---|---|---|
| `tyre` | `Tyre::addGroundContact` (vertical load: tyre spring and tyre damper summed), `Tyre::addTyreForcesV10` (grip: Fx and Fy as one world vector at the contact point; then Mz as a torque) | on the hub |
| `spring` | `Suspension::step`, first pair | spring plus packer rubber, summed; on the hub along the body's up axis, opposite on the body |
| `damper` | `Suspension::step`, second pair | damper force |
| `bumpstop` | `Suspension::step`, third and fourth pair | upper / lower bump stop (never reached in these scenarios) |
| `heave_spring`, `heave_damper`, `heave_bumpstop` | `HeaveSpring::step` | the full force on each of the two hubs and twice on the body |
| `arb` | `AntirollBar::step` | bar force on each hub and the body |
| `aero_drag`, `aero_lift` | `Wing::addDrag`, `Wing::addLift` | per wing, on the body |
| `sleep` | `Car::step` | `stop` on body and fuel tank while the car is frozen |
| `surface`, `drivetrain`, `brake`, `stability`, `steering`, `teleport`, `other`, `unknown` | | never occurred here: surface drag needs a surface with damping; the F2004's engine, drivetrain and brakes make **no** rigid-body call at all (wheel spin is a number inside tyre and drivetrain; the only reaction on a body is the tyre's grip force on the hub) |
| gravity, gyroscopic torque | inside `dWorldStep` | not on the tape: `solver.facc − facc` and `solver.tacc − tacc` |

Converter (full-precision text; f32 and f64 are printed in the shortest form that reads back to
the same bits). Without `--csv-out` the output is written next to the recording as
`<scenario>.<table>.csv`:

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
| Scenario | Steps | Two runs identical (hash of the recording) | Tyre forces = Rust tyre | Every tyre value = Rust tyre | Force calls explained | Top speed, km/h | Long. g (min … max) | Lat. g (min … max) | Smallest floor gap from step 250 on, mm |
|---|---|---|---|---|---|---|---|---|---|
| `settle` | 667 | yes (`13b66da193aaba92`) | 100 % | 100 % | 100 % | 3.4 | +0.00 … +0.00 | +0.00 … +0.00 | 24.7 |
| `settle_floor` | 667 | yes (`5db860c35ea11fcb`) | 100 % | 100 % | 100 % | 3.4 | +0.00 … +0.00 | +0.00 … +0.00 | 24.7 |
| `launch_autoclutch_off` | 4667 | yes (`60559aa7631e6031`) | 100 % | 100 % | 100 % | 285.1 | -0.95 … +1.62 | -0.00 … +0.01 | 5.3 |
| `launch_autoclutch_on` | 4667 | yes (`d401f7c0a2095866`) | 100 % | 100 % | 100 % | 285.1 | -0.92 … +1.60 | -0.00 … +0.01 | 5.3 |
| `brake` | 5334 | yes (`383c923eb4869a25`) | 100 % | 100 % | 100 % | 250.2 | -4.25 … +2.10 | -0.21 … +0.20 | 3.5 |
| `steady_corner_60` | 6667 | yes (`a378d64d17407982`) | 100 % | 100 % | 100 % | 64.5 | -0.49 … +1.35 | -0.29 … +0.00 | 23.8 |
| `steady_corner_120` | 6667 | yes (`7bf03ab7a70a12e6`) | 100 % | 100 % | 100 % | 123.8 | -0.49 … +1.47 | -1.16 … +0.00 | 19.7 |
| `steady_corner_180` | 6667 | yes (`227114b10567071d`) | 100 % | 100 % | 100 % | 182.6 | -0.49 … +1.54 | -2.57 … +0.00 | 12.8 |
| `slalom` | 5334 | yes (`1b6551554f8cf7b1`) | 100 % | 100 % | 100 % | 104.1 | -0.49 … +1.39 | -0.62 … +0.62 | 21.9 |
| `liftoff_oversteer` | 5334 | yes (`f435e9ca68c9cd15`) | 100 % | 100 % | 100 % | 182.6 | -0.71 … +1.54 | -2.72 … +0.00 | 10.9 |
| `kerb` | 4001 | yes (`f96c60d95f38e257`) | 100 % | 100 % | 100 % | 104.1 | -0.49 … +1.39 | -0.50 … +0.35 | 3.6 |
| `random` | 20001 | yes (`c2093b02da3b1773`) | 100 % | 100 % | 100 % | 222.6 | -2.63 … +1.66 | -1.29 … +1.29 | 10.4 |
| **all** | **70673** | | **282692 wheel-steps** | | **4586538 calls** | | | | |
<!-- RESULTS:END -->

How to read the columns:

- **Two runs identical**: each scenario was run twice, each time in a new process. The first
  run writes the file, the second only computes the hash; the value is an FNV-1a 64-bit hash of
  every byte of the recording (header, all steps, force tape, trailer), and "yes" means the two
  hashes are equal.
- **Tyre forces = Rust tyre**: share of wheel-steps in which every force call the tyre made on
  its hub and its `load`, `Fx`, `Fy`, `Mz` and `feedbackTorque` are bit-identical between the
  game and `VanillaTyre` fed with the recorded inputs. **Every tyre value**: the same for all
  220 recorded values per wheel-step (thermal patches, wear, flat spots, pressures …).
- **Force calls explained**: share of the calls on the force tape for which this holds bit for
  bit: the body's accumulators after the previous call on that body (zero at the start of the
  step), plus what ODE's own add-force arithmetic does with this call's vectors and the body's
  recorded pose, equal the accumulators recorded after this call. Two more checks are required
  to be 100 % for the table to be written at all: in every step the accumulators after the last
  tape entry equal the accumulators read at the entry of `dWorldStep` (nothing wrote to them
  unseen), and in every wheel-step the calls captured inside `Tyre::step` are exactly the
  tape's entries booked as `tyre` on that hub (kind, force bits, point bits, order).
- **Long. / lat. g**: range of the car's own `accG` after the first 1.2 s, ignoring steps in
  which the car is frozen by the sleeping rule.
- **Smallest floor gap**: 4.4.

What the tyre check proves, and what it does not. Fed to the Rust tyre from the recording: the
tyre's 60 inputs of each step (hub pose and velocities, brake torque, ground hit, car speed,
temperatures …) and, for the two driven wheels, the wheel speed and the wheel's spin matrix as
the drivetrain left them. The front wheels run free on the Rust side for the whole recording:
their spin is the Rust tyre's own from step 0 on. The Rust tyre is built the way the game
builds the car's tyres (`init` with the car present, the `reset` of the spawn, then
`setCompound`). So 100 % proves that the recorded tyre inputs and outputs belong together,
that the calls booked as `tyre` are exactly the tyre's, and that the Rust tyre is bit-exact
inside a whole car on these 70,673 steps. It proves nothing about the drivetrain (the rear
wheel speed is an input), the suspension or the brakes (hub pose and brake torque are inputs).
Branches these drives never reach are not tested by it either: a ray that misses the road,
surface drag, handbrake, ABS, electric motor torque.

Scenarios (all F2004, session-start setup, every scenario but the two `settle` ones first waits
1.2 s; the automatic start clutch is on except in `launch_autoclutch_off`; seed 1 except
`random`):

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

All twelve scenarios give the same hash in two fresh processes. What was needed for that is in
section 2: the fake clock, `srand`, the clock start, one process per scenario. No uninitialised
memory, thread or timer problem showed up in the recorded values.

Joint feedback was checked separately with the final build: `brake`, `settle_floor`, `kerb`
and `launch_autoclutch_off` were run again without the feedback buffers. All 2,116 values that
are not joint forces are bit-identical in every step, and so is the force tape. Asking ODE for
the joint forces does not change the simulation.

### 4.3 Sanity

| Check | Value |
|---|---|
| Mass | ODE bodies: car body 510 kg, fuel tank 59.2 kg (80 l), hubs 2 × 19 + 2 × 28.5 kg = 664.2 kg (605 kg car + fuel) |
| Weight on the wheels after 2 s | 1438 + 1438 + 1813 + 1813 N = 6,501 N = 663.0 kg. It is 1.2 kg short of the mass because the game's sleeping rule freezes the car from step 155 on, while it is still sinking; from then on it only creeps towards rest (wheel loads 5,161 N at step 154, the last step before the freeze, 6,441 N at step 399, 6,501 N at step 666) |
| Ride height after 2 s (the page's `rideHeight`) | 27.6 mm front, 54.7 mm rear; centre of gravity 0.234 m above the road |
| Standing start, times from the moment the throttle goes down | manual clutch: 100 km/h in 2.92 s, 200 in 5.28 s, 250 in 7.24 s; automatic clutch: 3.00 / 5.35 / 7.31 s. Counted from the first step at which the car is faster than 0.5 km/h instead: 2.42 / 4.77 s (manual) and 2.96 / 5.31 s (automatic) to 100 / 200 km/h. Sixth gear is engaged after 7.16 s at 248 km/h |
| Wheelspin at the start | manual clutch: rear slip ratio up to 8.1 (the wheels turn nine times road speed) until traction control first acts at 41 km/h (it works above 40 km/h only). Automatic clutch: slip ratio at most 0.3 |
| Top speed | 285.1 km/h at 18,800 rpm: the rev limiter in sixth gear (284 km/h is passed 9.65 s after the throttle went down). The scripts never use seventh gear, so this is not the car's top speed. Wheel loads there 3841 / 3841 / 5897 / 5895 N = 19,474 N, i.e. about 1,320 kg of downforce; ride height 9.5 / 21.8 mm |
| Braking from 250 km/h | 4.25 g at the first moment, 4.00 g mean over the first 0.3 s, 80.6 m and 2.73 s to below 1 km/h. Without ABS the front wheels are locked from 156 km/h down; the largest flat spot is 0.125 |
| Steady circle, 100 m (mean of the last 1.8 s) | 60 km/h: radius 100.0 m, 0.28 g, steering 0.147; 120 km/h: 100.0 m, 1.13 g, 0.163; 180 km/h: 99.9 m, 2.55 g, 0.234 (wheel loads 4012 / 740 / 4561 / 2273 N: the inner front is down to 740 N) |
| Lift-off at 180 km/h | throttle shut at 179.8 km/h; the yaw rate rises from 0.499 to 0.574 rad/s within 0.2 s (lateral 2.54 → 2.68 g, peak 2.72 g) and the car tightens its line without spinning |
| Kerb, 2 cm | the left front tyre's load jumps from 1,744 N to 7,398 N on the step up, the left rear's to 9,108 N 0.11 s later; the right front stays below 3,025 N. At the step down, 20 m on, the left front is in the air for 12 steps and the left rear for 8 (zero load) |
| Random minute | 222.6 km/h at most, −2.63 … +1.66 g longitudinal, ±1.29 g lateral, up to fourth gear |
| Engine and fuel | no engine damage in any scenario (life 1000 → 1000); 0.73 l of fuel in the 14 s manual launch, 1.34 l in the random minute |

Things that look odd and are the game's own behaviour:

- The −0.49 g "braking" that the table shows for the scenarios that never brake is drive-line
  judder while the game's automatic clutch closes at pull-away (step 829 in all of them:
  clutch 0.84, engine speed swinging between 4,800 and 5,580 rpm at 33 km/h). The manual start
  does not have it.
- The −0.95 / −0.92 g of the two launches is the rev limiter in sixth gear at 285 km/h: every
  time it cuts the engine for a few steps (engine torque −96 N·m instead of +284 N·m), air
  drag and engine braking slow the car at almost 1 g. The −0.71 g of `liftoff_oversteer` is
  the closed throttle at 178 km/h.
- At the end of `brake` the car, with locked wheels, overshoots zero speed, rolls back at up to
  3.9 km/h and rocks before the sleeping rule freezes it 2.99 s after the brakes went on. The
  +2.10 g and the ±0.2 g lateral in the table are that rocking, at walking pace after the car
  has first stopped (+2.10 g at 2.5 km/h while it rolls back, the lateral values at about
  1 km/h); during the stop itself the lateral value stays within 0.002 g.
- The spawn is not gentle: `Car::forcePosition` places the car above its rest height (body
  centre at 0.320 m, 0.234 m at rest). It falls; the rear tyres first carry load in step 29, the
  fronts in step 46, and the page's ride height goes from 140 / 120 mm to 27.6 / 54.7 mm.

### 4.4 The floor (where the oracle is not the game)

In the game the car body has six collision boxes (`colliders.ini`) that can rest on the track
mesh. The fake road has no mesh, so nothing ever stops the body; only the tyres' rays see the
road. `car_oracle check` therefore computes, from the recorded body pose and the six boxes, the
gap between the lowest box corner and the road for every step:

- **Steps 52 to 68 (the spawn drop):** the front floor box is below the road, at most 14.3 mm
  (15.7 mm in `launch_autoclutch_off`, the one scenario with the start aid off). This is also
  what the game does at a session start: its new-session job calls `setNoCollisionSteps(250)`,
  so for the first 250 steps (0.75 s) the engine does not look for contacts at all and the
  floor passes through the road there too.
- **From step 250 on** the gap is positive in every scenario (last column of the table); the
  smallest are 3.5 mm (`brake`, nose dive), 3.6 mm (`kerb`) and 5.3 mm (285 km/h). So in these
  twelve drives there is no moment at which the game would have had a body contact that the
  oracle lacks.
- **`settle_floor`** shows the other case, a car placed on the road in the middle of a session
  (nothing in the game's code calls `setNoCollisionSteps` after a teleport): with the mesh
  present ODE creates 4 contact joints in steps 53 to 64 (the engine's contact counter reads 4
  on every second of these steps, 24 in all), the dip stops at 9.9 mm instead of 14.3 mm, the
  car is frozen from step 147 instead of 155, and it ends at the same height (0.2336 m) with
  wheel loads within 1 N per wheel of `settle`. The contact joints themselves (their points,
  normals and forces) are not recorded; only their number is.

The default is contact-free on purpose: the next Rust stages have no collision code, and a
recording without contacts can be replayed by a solver that only knows the suspension rods.
`--floor` adds the mesh to any scenario. What is not exactly the game even then: the fake ray
returns the road height exactly, where the game's ray against a triangle mesh would return a
rounded hit point.

### 4.5 Things the runs settled

| Earlier open question | Answer from the oracle |
|---|---|
| Does a whole AC car run outside the game? (`car.md` 5.2 no. 2) | yes; `Car::Car` and the first step worked on the first try with the recipe of `body.md` 9.3 |
| Island and row order inside `dWorldStep` (no. 3) | bodies: hub RR, car body, fuel tank, hub LF, hub RF, hub LR (`tag` 0 … 5); joint blocks: fuel-tank joint, then the five rods of LF, RF, LR, RR in creation order (`tag` 0 … 20). Constant in all runs |
| Timing of the telemetry page (no. 5) | by the unpatched code: `velocity`, the angles, `localAngularVel` and `localVelocity` are the body **after** this step, `speedKmh` and `accG` are from **before** it. Example, step 1500 of the manual launch: `speedKmh / 3.6` = 32.24704 = the car's cached speed = the body's speed before the step; `|velocity|` = 32.28295 = the body's speed after it. The recorded Spa lap (made with the patched game) does not show this offset; why is not established here |
| Sign conventions (no. 11) | car axes: +x left, +y up, +z forward. Positive steering input turns right. In a right turn the yaw rate (`localAngularVel.y`) and the lateral `accG.x` are negative |
| Does the engine or the brake twist a body? | no: on this car no call on a rigid body comes from the drivetrain, the engine or the brakes |
| Is asking ODE for joint forces harmless? | yes (4.2) |
| Tyre in the car versus tyre on the rig | identical once two things are treated as inputs for the driven wheels: the wheel speed and the wheel's spin matrix, both rewritten by the drivetrain between tyre steps |
| What does a session start do to the car? | the default compound (not index 0) and the setup round trip of 2.6; both must be part of any Rust "new car" before it is compared with the game |

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
| Rust tyre differed for the rear wheels (`localWheelRotation` only) | the drivetrain turns the driven wheels' spin matrix after the tyre step | recorded on entry and fed to the Rust tyre as an input (driven wheels only) |
| Floor boxes below the road on a 3 cm kerb | no collision mesh (4.4) | kerb lowered to 2 cm; gap reported per step by `check`; `--floor` option |
| Quaternion columns were labelled x, y, z, w | ODE's order is w, x, y, z | named explicitly |

Found by the three reviews after the first full set of recordings, and fixed (all recordings
were then made again; the numbers in this report are from the final set):

| Problem | Cause | Fix |
|---|---|---|
| The car ran on the wrong tyres (Slick Soft) | the constructor leaves compound 0; in the game the setup screen selects `[COMPOUND_DEFAULT] INDEX` at session start | 2.6, step 1 |
| Setup values were the raw file values | the setup screen's round trip was missing (camber 3.0° instead of the game's 2.9°, …) | 2.6, step 2, checked against a real game log |
| A stray 1st → 2nd → 1st shift at pull-away in the scripts | a paddle counter left over from the start sequence | cleared when first gear engages |
| The `tape` table booked hub calls as `other` | it used the inner call site | booked by the outer site, as the `sum.` columns already were |
| The "tape adds up" check was weaker than its name | it only compared the last entry of each step with the accumulators | the per-call chain test ("Force calls explained"), which recomputes every call with ODE's arithmetic |
| The g range in the table showed artefacts of frozen steps | `accG` of steps in which the body is stopped by hand | those steps are left out of the range |
| `car_oracle all --car <other car>` recorded the F2004 and then checked it against the other car's data | `all` did not hand `--car` to the runs it starts (found by the final fact-check) | it does now |
| Logger: `--to-csv --out` with the input's own name destroyed the recording; a recording stopped before its first packet had no valid header; a page could be read while the game was half-way through copying it | | refuses to overwrite its input and checks the header before creating anything; a valid header is written when the file is created; each page is read until two reads agree (up to 20 tries, failures are counted and reported) |

Not done:

- The tyres' ground rays go through the track's `rayCast` entry (the path the game's own tyre
  test bench uses), not through a per-wheel ODE ray against a mesh. In `Tyre::step` both paths
  fill the same hit point, normal and surface and join at 0x140283b2d, so the tyre maths is the
  same; what differs is who finds the hit. The fake answers like the game's 3 m ray: a hit when
  the road is at most 3 m below the ray's start (2 m above the wheel centre).
- No run of the car without the recording hooks exists to compare with. The hooks pass every
  argument on untouched and the joint feedback was shown to be harmless, but "hooked equals
  unhooked" is argued, not measured.
- The setup round trip covers the `SetupManager` items that have a section in `setup.ini`.
  Whatever else the setup screen's other tabs (gear ratios, electronics) may do at a session
  start is not mirrored.
- Contact joints of `--floor` runs are counted, not recorded (4.4).
- Only double-wishbone cars: the harness stops on any other suspension type (the hub pointer
  and joint list are read from the `Suspension` layout).
- The graphics and static telemetry pages are not produced (they need the renderer-side
  objects); only the physics page is.
- Bump stops, surface drag, ABS, EDL, DRS, KERS, seventh gear, reverse and the handbrake never
  act in these twelve runs, and collisions act only in `settle_floor` (counted, not recorded),
  so there is no recorded example of them.
- The recording's header does not identify the `msvcr120.dll` or the CPU that produced it
  (7.2 no. 1).

---

## 6. Commands

```
cargo build --release --manifest-path tools/car_oracle/Cargo.toml

car_oracle list                                   # the scenarios
car_oracle all                                    # every scenario twice + all checks, writes oracle/car/results.md
car_oracle all --only brake,kerb                  # the same for some scenarios (results.md then has only these)
car_oracle run --scenario brake                   # one scenario -> oracle/car/brake.carrec, prints its hash
car_oracle run --scenario brake --floor           # with the collision mesh -> brake_floor.carrec
car_oracle run --scenario brake --no-joint-forces --out re/scratch/nojf   # without ODE joint feedback
car_oracle run --scenario random --steps 3000 --hash-only
car_oracle check oracle/car/brake.carrec          # tyre replay, tape checks, headline numbers, floor gap
car_oracle setup-check [--car bmw_z4_gt3]         # the game's own "Setup change for Car" lines for a session start
car_oracle diff a.carrec b.carrec [--only prefix,prefix] [--ignore .f1.,.t1.,.f2.,.t2.]
car_oracle csv oracle/car/brake.carrec --table steps --only step,controls.,body.,sum.body. --every 10
car_oracle csv oracle/car/brake.carrec --table tape --from 3000 --to 3010
car_oracle csv oracle/car/brake.carrec --table telemetry --csv-out brake_page.csv
```

(Run from the repository folder; `car_oracle` stands for
`tools/car_oracle/target/release/car_oracle.exe`. Recording paths are relative to where the
command is run; the default output folder `oracle/car` and the scratch game folder are those
of the repository the tool was built in. `run` writes `<out>/<scenario>.carrec` and replaces a
file of that name, so give variants their own `--out`. Common options: `--acs <path to acs.exe>`, `--out <dir>`,
`--root <scratch game folder>`, `--car <folder under cardata/>` (double-wishbone cars only),
and with `run`, `--verbose` to see the game's own console output. `all` ends with an error if
any check of any scenario is below 100 %.) `python tools/car_oracle/gen_functions.py` regenerates
the embedded function table from `re/index` (only needed if the index changes). Do not rebuild
while a run is in progress: `all` starts the same executable again for every scenario.

Telemetry logger v4:

```
python ac_telemetry.py --laps 3 --print --out lap.csv            # as before: rounded CSV, same columns as v3
python ac_telemetry.py --raw --laps 3 --print --out lap.acraw    # whole physics + graphics page per packet, static page once
python ac_telemetry.py --to-csv lap.acraw [--out lap.csv]        # full-precision CSV (+ lap.static.csv); no game needed
```

`--laps N`, `--duration S` (default 900 s, 0 = no cap) and `--print` work in both modes. The raw
file is: `"ACTLMRAW"`, version 4, header size, the three page sizes (592, 304, 684), record
size, start time, the static page; then per packet an f64 time and the two pages, bit for bit.
The converted CSV starts with the v3 columns (unrounded), followed by every other field of the
two pages. The logger stays read-only on the shared memory; it reads each page until two reads
agree, because the game copies the page without a lock, and reports how many pages never did.
It was tested without the game by two scripts in `re/scratch/car_oracle/`.
`test_logger_v4.py` plays the game's part (it creates the three shared-memory pages and keeps
rewriting them while the logger records): every recorded page is byte-identical to a page that
was written, and the rounded CSV still has the v3 columns. `test_logger_roundtrip.py` writes a
raw file of 300 packets filled with random finite float bit patterns (plus edge cases such as
−0, the smallest and largest numbers), random ints and texts, converts it, and compares every
value of every row with the raw bytes: all 42,600 floats, 10,500 ints and 1,500 texts read back
exactly. (A NaN would be written as `nan` and lose its payload bits.) The logger has not yet
been run against the live game.

---

## 7. Open questions

### 7.1 Choices made while working unattended

1. **The car is set up as at a session start** (2.6): default compound from
   `[COMPOUND_DEFAULT]`, setup values after the setup screen's rounding. The alternative, the
   raw values of the data files, is what the constructor gives and is not what the game drives.
2. **Contact-free by default** (4.4). The alternative, the collision mesh in every scenario,
   is one flag away; say if the goldens should be made that way.
3. **Start state**: the game's own spawn (with its drop) followed by 1.2 s of rest, not a
   hand-made rest pose.
4. **Weather and session**: 26 °C air, 30 °C road, no wind, no session, damage and wear rates
   at the game's 100 %, default fuel (80 l), blankets on. Auto-blip and traction control stay
   as the car's files set them (both on). "Auto-clutch off" clears only the start aid; the
   clutch dip on down-shifts stays, as it does in the game for this car (`USE_ON_CHANGES=1`).
5. **The physics clock starts at 60 s**, not at 0.
6. **Two helper objects of the player car are switched off** (`PerformanceMeter`, `Telemetry`).
   They do not touch the car's motion.
7. **Joint forces are on in the goldens** (ODE feedback), after showing that it changes nothing.
8. **Scripts are closed-loop** (speed holding, yaw-rate steering, shifting by road speed). They
   only read the car's state of the previous step, so the runs are still deterministic, but a
   Rust car that differs slightly will be driven slightly differently. For a strict
   comparison replay the recorded `script.*` columns instead.
9. **`kerb` uses a 2 cm strip** (3 cm made the floor touch it), and `brake` has no ABS variant
   because the F2004 has no ABS.
10. **`launch` stops at sixth gear**, as the task says; seventh is never used.
11. **Recordings are big** (13.8 kB per step, 0.97 GB in all) and stay outside git; nothing
    compact is checked in yet. The hash list is in the table above.
12. **Subagents**: six read-only research agents wrote hand-over notes from the disassembly
    (three at a time), three read-only reviewers checked the first full result, and two
    read-only fact-checkers checked the final text of this report against the recordings and
    the code (their eight corrections are in); none committed. A seventh research agent (the
    tyre inside the car) was stopped when the measurement itself had answered its question.
    For about a minute four agents were running at once (that seventh one and the three
    reviewers), one more than the limit of three, before it was stopped.
13. **Logger v4**: `--raw` is a switch that changes the output format (default extension
    `.acraw`); the converted CSV starts with the v3 columns so `check_telemetry.py` reads it.
14. **`.gitignore`**: the rule `oracle/` also hid `docs/oracle/`; it is now `/oracle/`.
15. **`docs/map/car.md`** got a short section 5.2a listing what this task answered.

### 7.2 Questions

1. Are the recorded hashes the same on another PC? The game code is the same, but `sinf`,
   `cosf`, `powf` and friends come from the installed `msvcr120.dll`, which may pick a different
   code path on another CPU.
2. Should the first Rust rigid-body test replay from step 0 (through the spawn drop, with the
   setup applied in step 0) or from a later step? Both are possible: every step records the
   full state before and after.
3. The gyroscopic torque that ODE adds inside the step is now recorded as a difference
   (`solver.tacc − tacc`), not derived. Its formula still has to be read from the code for the
   port (`physics_engine.md` describes it from the ODE source).
4. A lap from the unpatched game recorded with `ac_telemetry.py --raw` could now be laid next
   to the oracle's `telemetry` table (same columns) to see the one-step offset of 4.5 in the
   real game.
5. One F2004 session in the real game would leave its "Setup change for Car" lines in
   `log.txt`; comparing them with `car_oracle setup-check` would confirm the 11 values of 2.6
   directly (so far only the Z4 GT3's 24 lines are confirmed).
6. Other cars (strut, axle, multilink suspensions; turbo, KERS, ABS) need small additions to
   the harness (hub and joint discovery per type) before they can be recorded.
