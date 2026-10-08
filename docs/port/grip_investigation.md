# Task 16, Step 0: why does rustyAC feel grippier than my Assetto Corsa?

Desk work only: nobody drove. Everything here is from files, the disassembly, the oracle and runs
rustyAC can do alone. The briefs behind it are in the git-ignored `re/scratch/task16/`
(`csp_survey.md`, `oracle_vs_session.md`).

## 1. In plain English

- **rustyAC is the car of plain `acs.exe`. Your Assetto Corsa is not plain `acs.exe`.** Custom Shaders
  Patch (yours is 0.3.0-preview649, build 4184) changes physics for a stock Kunos car even with
  "extended physics" off, and none of that can be switched off except by switching CSP off.
- **Nothing was found that is wrong in rustyAC's physics or in how it starts a session.** Every value
  that reaches the tyres at the start is the same as in the game's own start code: compound, 80 C
  tyre blankets, 10 psi cold, 80 l of fuel, traction control at level 4 of 10, no ABS, 55 % brake
  bias, track grip 100 %, 14 / 20 C.
- **Two CSP changes are proven from your own recording and both take grip away from your AC:**
  1. thinner air (known since Task 15): 5.6 % less downforce and drag at the Bus Stop;
  2. **new: another tyre pressure law.** In your AC the pressure follows a gas law from 10 psi at the
     day's 14 C; `acs.exe` adds a fixed 0.16 psi per degree of tyre temperature from 26 C. Hot tyres
     in your AC have **about 6 psi less** than in rustyAC (18.8 against 24.9 psi at the Bus Stop on a
     flying lap). The F2004's tyres want 23 psi: with the game's formulas that is 1 to 2.4 % less
     grip and a much softer tyre in your AC.
- **Both together are small at the Bus Stop**: about 2 % of the speed the corner can be taken at
  (section 5). They do not explain "I'd spin where rustyAC is almost flat" by themselves.
- **The lap we compared with so far cannot be used for this.** `f2004_spa_ai.csv` is the game's AI
  driving. The game gives an AI car 20 % more tyre grip whenever it is off the throttle (the F2004's
  `ai.ini [ULTRA_GRIP] VALUE=1.2`), switches its traction control and ABS off and gives it a strong
  stability aid. That lap shows 2.5 to 2.9 g in the Bus Stop where a player's F2004 tops out at
  about 2.4 g in rustyAC. So Task 15's "rustyAC has more grip than the recording" was the wrong way
  round for that reason alone: the recording has *more* grip than any player's car.
- **What is big:** traction control. With it switched off the test driver cannot even finish a lap
  of Spa in rustyAC; with it on (level 4, as in your AC by default) the exit of the chicane is easy.
  Your `assists.ini` says traction control "as the car has it", so both should be the same, but it
  is the first thing to check in a lap you drive yourself (the `tc` column: 0.08 when it works).
- **The best remaining suspect for the kerbs is CSP's collision handling.** In plain AC and in
  rustyAC only six soft "floor boxes" of the F2004 can touch a kerb. CSP has an option that is on
  by default, "Enable cars mesh collider sooner (activates proper car-ground collisions when car is
  not driving straight)", and several hidden always-on changes to car/ground contacts, all running
  on CSP's own physics engine (its own build of ODE in double precision). A hull that strikes a
  kerb takes load off the tyres: that is what "I'd spin on the kerbs" looks like. It is not proven;
  one run settles it (section 7, run 2).
- **No fix was made to rustyAC's physics**, because no fault was found in it. Three small things on
  rustyAC's side were found and are listed in section 6 (none changes grip); two experiment
  switches and a measuring tool were added so that you can test the CSP items by feel.

## 2. The suspects, ranked

By how likely each is to be what you feel at the Bus Stop.

| # | Suspect | Side | Evidence | Size at the Bus Stop | How to prove it |
|---|---|---|---|---|---|
| 1 | **Body against kerbs handled differently by CSP**: the car's mesh may touch the ground "sooner" (`general.ini [PHYSICS_EXPERIMENTS] ENABLE_MESH_COLLIDER_SOONER=1`, default, yours), new collision logic and rim collision (hidden, "enabled everywhere now"), CSP's own double-precision ODE | CSP | CSP's own option texts and its log (`ODE_double_precision`, `ODE_EXT_libccd`); the F2004's floor is 27 mm (front) / 54 mm (rear) off the road; plain AC lets the mesh touch the road only when the car lies on its side | unknown, can be large: in rustyAC a hull that may always touch the ground does not survive a lap of Spa (it stops on a kerb at 51 % of the lap), which says how violent a hull contact is, not what CSP's rule is | run 2 (CSP off) and run 6 (that one option off, it can be changed live) |
| 2 | **Traction control not the same in the two drives** | you / session | with TC off the line follower cannot lap Spa at any pace; with TC on the chicane's exit is easy. Both programs start with it on at level 4 (slip limit 0.08): checked in the code and in the standing rows of your recording | very large if it differs | run 1: the `tc` column of a lap you drive (0.08 while it is switched on) |
| 3 | **CSP's tyre pressure law** (new) | CSP | your recording: `pressure = 10.97 + 0.084 x (core - 26)` on all four wheels within 0.03 psi over three laps; plain `acs.exe` and rustyAC: `10 + 0.16 x (core - 26)` exactly | 1 to 2.4 % of tyre grip, a 22 to 30 % softer tyre; the Bus Stop limit drops from 72.1 to 71.5 km/h, 2.38 to 2.35 g | run 2 must show 18.64 psi at 80 C; rustyAC can copy it: `--pressure-law 10.97,0.084` |
| 4 | **CSP's thin air** (Task 15) | CSP | `airDensity` 1.157 .. 1.172 against 1.2348 | 0.5 % of tyre load at 64 km/h, 1.1 % at 100, 3.9 % at 310; limit 72.1 to 71.6 km/h | run 2 must show 1.2348; rustyAC: `--air-density 1.165` |
| 5 | **A long drive against a short try**: after some laps your real tyres are at 113 to 124 C and a little worn; a fresh rustyAC start is at 80 C on new tyres. The F2004's grip is flat from 70 to 115 C, so this is small | you | tyre data (`tcurvemedium.lut`), your recording | under 1 % | compare tyre temperature, pressure, wear in both recordings before comparing speeds |
| 6 | **rustyAC's copy of your pad settings is old**: `target\release\rustyac_controls.ini` has `STEER_DEADZONE=0.1`, AC's `controls.ini` has 0.05 now | rustyAC (settings file) | both files | none on grip; the wheel answers a little later around the centre in rustyAC | delete the copy (it is written again from AC's file) |
| 7 | The setup screen's "round trip" (camber 3.0 to 2.9 degrees and ten more values) may not have happened in your last real F2004 session: its log has none of the eleven lines plain `acs.exe` prints for it | unknown (Content Manager / CSP) | your log of 7 Oct | under 0.3 % either way | the standing row of run 1 (camber) |
| 8 | Wind: drawn anew at every start in both | both | code | about 1 % of the air speed | `--wind-from-log` |
| 9 | A kerb mesh that the port's track reader places or picks differently from the game's | rustyAC (shared with the oracle) | not seen: the road under the AI's line is within 13 mm for 99 % of three laps, and the Bus Stop kerbs are driven in the bit-exact recordings | unknown, unlikely | run 4 laid over run 3 with `compare_laps.py --plot` |

## 3. What is ruled out

- **rustyAC's physics against plain `acs.exe`:** bit-exact (all earlier tasks), including the Bus
  Stop kerbs with body contact (`spa_kerbs`, `spa_kerb_strike`).
- **The session start** (section 6): no difference that reaches the tyres.
- **The AI/player switch:** rustyAC has none of the AI's help. `Tyre::aiMult` is 1.0 for ever in the
  port; the oracle's driver object is not an `AIDriver`.
- **CSP's gamepad script:** off (`gamepad_fx.ini [JOYPAD_ASSIST] ENABLED=0`, the default; you have
  no `gamepad_fx.ini`). Your pad goes through `acs.exe`'s own code with `STEER_SPEED=0.95`,
  `STEER_GAMMA=1.4`, `STEER_FILTER=0`, `STEER_DEADZONE=0.05`, `SPEED_SENSITIVITY=0`: full lock in
  33 ms, no speed sensitivity. rustyAC has the same code (see suspect 6 for the one number).
- **CSP's "extended physics":** off for the F2004 at Spa (car `VERSION=1`, track `WAV_PITCH=0`,
  `EXTENDED_PHYSICS=0`, no new game mode).
- **Rain, "Surfaces FX", physics shadows, steering extrapolation, input remaps:** off or dry.
- **CSP's car and track config for the F2004 and Spa:** looks only (lights, materials, sparks).
- **Tyre blankets, track grip, temperatures, fuel, compound, brake bias, damage:** the same.
- **The stability aid:** 0 in both; at 100 % it changes nothing in the Bus Stop limit anyway.
- **The pad's "automatic clutch" setting:** the game forces it on for a pad whatever `assists.ini`
  says, as rustyAC does.

## 4. What CSP changes for a Kunos car with extended physics off

"Yours": the value in your installation (your `cfg\extension` holds only looks settings, so
everything is CSP's default). Full table with every key: `re/scratch/task16/csp_survey.md`.

| What | Setting | Yours | Effect on grip |
|---|---|---|---|
| Air density falls with the car's altitude (and CSP's humidity / pressure): `1.1677 x exp(-height / 8415 m)` fits your lap | none (always on) | on | less downforce and drag, 5.6 % at the Bus Stop |
| Tyre pressure follows a gas law from the cold pressure at the air's temperature | none found in any text file (found in the recording) | on | less: lower pressure when hot |
| The rigid-body engine is CSP's own ODE build (double precision, libccd, threads); `acs.exe`'s is single precision | no visible key | on | none systematic; never bit-comparable |
| AC's own physics thread pool is switched off; CSP runs its own workers | `[THREAD_TWEAKS]`, hidden keys | on | none |
| Tyre contact points found with Intel Embree instead of ODE rays | `general.ini [PHYSICS_EXPERIMENTS] ODE_BATCH_RAYCAST=0` | Embree | unknown, small: which of two overlapping surfaces wins |
| Car mesh may touch the ground "sooner" | `ENABLE_MESH_COLLIDER_SOONER=1`, `CLAMP_MESH_COLLIDERS=1` | on | **unknown, possibly much less on kerbs** |
| Car box colliders extended; new collision handling and checks; rim collision; "sliding fix"; "brakes limit" | `EXTEND_COLLIDER_BOXES=1`; hidden `[PHYSICS_EXPERIMENTS_DONE]` ("enabled everywhere now") | on | unknown (no description in CSP's files) |
| Off-track surfaces: Perlin noise instead of the sine bumps | hidden | on | none on asphalt and kerbs (Spa: `SIN_HEIGHT=0` there) |
| A rain compound added to every car (the F2004 has four compounds under CSP) | RainFX | on | none (the car still starts on Slick Medium) |
| The first lap from a hot-lap spawn counts in a practice session too | hidden `ARM_FIRST_LAP=1` | on | timing only |
| A clutch button for pads (`[__EXT_KEYBOARD_CLUTCH]`), yours: A | controls.ini | bound | none unless pressed |
| Gamepad script, steering extrapolation, input remaps, surfaces FX, physics shadows, extended physics | their switches | **off** | - |
| The AI: lateral grip estimate, the tyre help for AIs (`FIX_GRIP_MULT`), brake target, steering | `[CONTROL]`, `new_behaviour.ini [AI_TWEAKS]` | on | none for you; the AI lap is not plain AC's AI either |

## 5. Numbers

### 5.1 The Bus Stop in your recording and in rustyAC

`python tools\corner_numbers.py <csv>` prints these for any recording. `f2004_spa_ai.csv`: the
game's AI under CSP, three laps. rustyAC: the line follower of `grip_probe` at the fastest pace
that still makes the corner (plain-AC air and pressures; its first lap is slow, so its tyres are
cooler).

| | Real AC, AI, flying laps | rustyAC, line follower at its limit |
|---|---|---|
| speed in / minimum / out, km/h | 310 / 64 / 152 | 294 / 72 / 100..159 |
| gears | 8 down to 1 | 7 down to 2 |
| braking g (10 m mean) | 4.54 | 3.27 (the follower brakes early) |
| lateral g (10 m mean), left / right | 2.5 / 2.7 to 2.9 | 2.38 / 2.13 |
| tyre force per newton of load, 99 % of the rows below | **2.38** | **1.93** |
| tyre core temperatures, C | 113 to 123 | 77 / 79 / 95 / 95 |
| tyre pressures, psi | 18.2 to 19.1 | 18.1 to 18.5 front, 20.9 to 21.1 rear |
| the same tyres by plain `acs.exe`'s law | 23.9 to 25.1 | (as shown) |
| ride height front / rear, mean, mm | 24 / 50 | 25 / 51 |
| air density | 1.166 | 1.2348 |
| traction control in the telemetry | 0 (the AI has it off) | 0.08 |

What stands out:

1. **The AI makes 23 % more force out of the same tyre load** (2.38 against 1.93). 1.2 x 1.93 = 2.32:
   that is the AI's `ULTRA_GRIP` and nothing else. Read from the code: `AIDriver::stepGasBrake`
   0x1402a08e0 writes `Tyre::aiMult` of all four tyres every step, 1.001 on the throttle, `ultraGrip`
   (1.2, `AIDriver::AIDriver` 0x140298c60) otherwise; `Tyre::addTyreForcesV10` multiplies the side
   force by it and switches to a simpler tyre model when it is above 1.
2. **The pressures**: the real tyres are 40 C hotter and still at the pressure rustyAC's tyres have
   at 77 C.
3. The AI takes the apex slower (64 km/h, first gear) than the car can do; it is no measure of the
   limit either way.

### 5.2 The Bus Stop with one thing changed at a time

`grip_probe` (section 8) drives a lap at a safe pace, then takes Blanchimont's exit to the line at
a higher "pace" (the share of the grip it plans its corner speeds with) and raises it in steps of
0.02 until the car spins or leaves the track. One step is about 1 % of corner speed. Baseline: the
conditions of your race.ini without the random parts (14 / 20 C, grip 100 %, no wind), default
setup, traction control as the car has it.

| Changed | Last pace that works | Minimum speed, km/h | Peak lateral g | Says |
|---|---|---|---|---|
| nothing (baseline) | 1.26 | 72.1 | 2.38 | |
| air density 1.165 (CSP) | 1.24 | 71.6 | 2.36 | one step |
| CSP's pressure law | 1.24 | 71.5 | 2.35 | one step |
| both | 1.24 | 71.6 | 2.33 | still one step: together about 2 % |
| track grip 98 % | 1.22 | 70.9 | 2.32 | the scale: 2 % of grip = two steps |
| track grip 96 % | 1.20 | 70.2 | 2.26 | |
| **traction control off** | **none** | - | - | **the car does not get round its first lap** |
| stability aid 100 % | 1.26 | 72.1 | 2.38 | nothing |
| tyre blankets off | 1.26 | 72.1 | 2.38 | nothing after a lap |
| wind 10 km/h from 0 / from 180 degrees | 1.24 / 1.28 | 71.5 / 72.8 | 2.38 / 2.39 | one step either way |
| fuel 20 l / 110 l at the start | 1.30 / 1.24 | 73.4 / 71.5 | 2.43 / 2.36 | |
| cold pressures +3 psi / -3 psi (plain law) | 1.26 / 1.22 | 72.1 / 70.9 | 2.38 / 2.31 | low pressure costs grip |
| 26 C air, 30 C road (rustyAC before Task 15) | 1.26 | 72.2 | 2.39 | nothing |
| wheel turned at most 1.5 / 0.75 locks per second | 1.22 / 1.12 | 70.9 / 67.6 | 2.20 / 2.16 | a slow wheel costs more than CSP's air (your pad: 30 locks per second) |
| the car's mesh may always touch the ground | none | - | - | stops on a kerb at 51 % of the first lap: too crude to stand for CSP's rule |

## 6. The oracle against a real session start

What `car_oracle` sets itself instead of going through the game's session start, held against the
start chain of `acs.exe` for the player (`Sim::Sim`, `RaceManager::initOffline` 0x14013a6c0,
`setCurrentSession`, `CarAvatar`, `DrivingAssistManager`, the setup screen) and against what
`rustyac.exe --track spa` does. Full table with addresses: `re/scratch/task16/oracle_vs_session.md`.

| State | The game, for the player | `car_oracle` | `rustyac.exe` | Difference |
|---|---|---|---|---|
| Spawn | pit box, then the session's spawn set (yours: `HOTLAP_START`), snapped to the road | the port's track reader gives the point, the game's `forcePosition` places the car | the same functions | none in the car's state |
| Tyre temperatures, blankets | the air's; blankets hold 80 C until 10 km/h (`assists.ini TYRE_BLANKETS`) | the game's code, blankets on | from assists.ini | none |
| Tyre pressures | the compound's static pressure (10 psi), setup items write it back; dynamic = `(core - 26) x 0.16 + static` | game code | bit-exact | none (your AC with CSP: section 4) |
| Compound | `[COMPOUND_DEFAULT] INDEX` (1, Slick Medium) | set by hand with that index | the same | none |
| Wear, flat spots, grain, blisters, dirt | zero | zero | zero | none |
| Fuel | `car.ini [FUEL] FUEL` (80), cut to whole litres by the setup screen | 80, not cut | 80, not cut | a car whose file has a fraction starts with it (none known) |
| Track grip, dynamic track | `[DYNAMIC_TRACK]`, drawn at the session start | the game's own function on a race.ini the oracle writes | the same draws | none |
| Air and road temperature | `[TEMPERATURE]` | two numbers written into the engine | from race.ini | none |
| Wind | drawn twice with `rand()`, swings with the time since the session began | the game's job on a hand-made object | drawn anew at every start | the draw; **the swing's phase (D1 below)** |
| Session start, gearbox lock, lights | `startTime` = the clock at the session's start; type 1 practice, 4 hot-lap | nothing: session info stays zero | start time 0 with a clock that starts at 60 s; type 4 for `--spawn hotlap` | **D1**, **D3** |
| Damage | zero at the spawn; rate from `assists.ini DAMAGE` (yours 0) | zero; rate 1 | from your file | none |
| ABS, traction control, stability | `assists.ini`: 0 off, 1 as the car, 2 forced; stability percent | the car's own files | applied | none |
| TC / ABS level | TC: the row of `traction_control.lut` equal to `SLIP_RATIO_LIMIT`: level 4 of 10, slip 0.08 | game code | ported | none |
| Automatic clutch | on when asked for or when the device is a pad or the keyboard | written by hand per scenario | on unless `--no-auto-clutch` | none for a pad (Step 3 of this task applies the file for a wheel) |
| Automatic blip | `assists.ini AUTO_BLIP` | left on | left on | **D2**: cars without an electronic blip (not the F2004) |
| Automatic gearbox | `assists.ini AUTO_SHIFTER` | per scenario | the `--auto-shifter` flag | none for you |
| Brake bias, setup defaults | from the car's files; every setup item goes once through its whole-number spinner (camber 3.0 to 2.9 ...) | mirrored by hand (checked against a real log of another car) | the same mirror | none against plain AC; suspect 7 |
| Ballast, restrictor, penalties | race.ini | setters / by hand | from race.ini | none |
| First lap armed | only in a hot-lap session | per scenario | always for `--spawn hotlap` | **D3** (timing only; CSP arms it in practice too) |
| AI or player | the player's device is never an `AIDriver` | a fake device that is not one | no AI | none |
| The controls device | the pad class with your `[X360]` values | a script; no device code runs | the ported pad class reading its own copy of your file | **D4** |
| Physics rate | 333.33 Hz | the same | the same, checked without drift | none |
| The track: which meshes are physical, their surfaces | the game's kn5 reader | the game's `Track` fed by **the port's** kn5 reader | the same reader | shared; checked only by laying a real lap over it (suspect 9) |

Not exercised by any oracle recording, but by a real drive: many laps in a row (tyres from 80 to
120 C, wear along the curve, flat spots piling up; the single-tyre oracle covers the tyre code, not
the whole car over laps), the real controls classes, the game's own track reader, the session code
itself.

The definite differences between `rustyac.exe` and plain `acs.exe`. None changes grip:

- **D1** the session's start time is 0 while the clock starts at 60,000 ms: the wind's slow swing is
  60 s ahead and the automatic gearbox does not wait its 300 ms. Fixed in Step 3 of this task.
- **D2** `assists.ini AUTO_BLIP` was not applied (an F40 blips in rustyAC where your AC with
  `AUTO_BLIP=0` does not). Fixed in Step 3.
- **D3** `--spawn hotlap` armed the first lap whatever the session's type. Fixed in Step 3.
- **D4** `target\release\rustyac_controls.ini` is a copy of your controls made on 7 Oct; it says
  `STEER_DEADZONE=0.1` where AC's file says 0.05 now. Delete it to copy afresh.

## 7. The telemetry runs for you

Everything is recorded with `ac_telemetry.py` (it reads the shared memory of whichever program
runs; only one of the two at a time). Same car (`ks_ferrari_f2004`), Spa, default setup, the same
session as now (14 / 20 C, grip 100 %), fresh session each time, **you driving**, three flying laps.
Make the folder once: `mkdir re\scratch\laps`.

| Run | Do | Command | Look at | It proves |
|---|---|---|---|---|
| **1. Real AC, CSP on** (as you always drive) | start the recorder, then AC, drive 3 laps, close AC | `python ac_telemetry.py --laps 3 --out re\scratch\laps\ac_csp_on.csv` | `python tools\corner_numbers.py re\scratch\laps\ac_csp_on.csv --pressure` | `tc` must read 0.08 in the Bus Stop rows: if it reads 0, **traction control is off in your AC and that is the whole difference** (suspect 2). The pressure fit must say `10.97 + 0.084`: CSP's law. `airDensity` 1.166 |
| **2. Real AC, CSP off** | Content Manager, Settings, Custom Shaders Patch: untick "Active". Then as run 1 | `python ac_telemetry.py --laps 3 --out re\scratch\laps\ac_csp_off.csv` | the same command on this file | `airDensity` 1.2348 and pressure `10 + 0.16`: you are now in plain AC. **If the Bus Stop now feels like rustyAC, the difference is CSP** (suspects 1, 3, 4). **If you still spin, it is not CSP**: then run 4 against this one is the comparison that counts |
| **3. rustyAC as it is** | first delete `target\release\rustyac_controls.ini` (suspect 6); start rustyAC, then the recorder in a second console, drive 3 laps | `target\release\rustyac.exe --track spa --windowed --wind-from-log` and `python ac_telemetry.py --laps 3 --out re\scratch\laps\rusty.csv` | `python tools\compare_laps.py re\scratch\laps\ac_csp_off.csv re\scratch\laps\rusty.csv --name-a AC --name-b rustyAC --plot re\scratch\laps\off_vs_rusty.html` | against run 2 this is plain AC against rustyAC with you driving both: the rows "Bus Stop 1 / 2" (minimum speed, peak lateral g, steering) should agree as far as two laps of yours agree with each other. A gap here would be a real fault of rustyAC |
| **4. rustyAC with CSP's air and pressures** | as run 3 | `target\release\rustyac.exe --track spa --windowed --wind-from-log --air-density 1.165 --pressure-law 10.97,0.084` and `python ac_telemetry.py --laps 3 --out re\scratch\laps\rusty_csp.csv` | `compare_laps.py` against `ac_csp_on.csv` | rustyAC with the two proven CSP changes. What is still different from run 1 is the rest of CSP: the collisions (suspect 1) |
| **5. Traction control by feel** | in rustyAC press the pad's D-pad down (or Alt+Shift+T) four times: TC off | `target\release\rustyac.exe --track spa --windowed` | how the exit of the Bus Stop feels | if *that* is "real AC", your AC runs without traction control |
| **6. One CSP option** | CSP on. Content Manager, Settings, Custom Shaders Patch, "Experiments": untick "Enable cars mesh collider sooner" (it can be changed while driving); drive the Bus Stop over the kerbs; tick it again. Then the same with "Extend car box colliders" off, and with "Use ODE for multiple contact point lookups" on | - | whether the kerbs stop upsetting the car | the option whose change makes the kerbs harmless is the cause (suspect 1) |
| **7. Standing still** (30 seconds, no driving) | real AC with CSP on, 5 s on the start point; then rustyAC | `python ac_telemetry.py --duration 5 --out re\scratch\laps\ac_rest.csv`, then `target\release\rustyac.exe --track spa --headless --duration 6` with `python ac_telemetry.py --duration 5 --out re\scratch\laps\rusty_rest.csv` | first rows of both: `wheelsPressure` (15.52 against 18.64), `rideHeight`, `camberRAD`, `wheelLoad` | the pressure law without driving, and whether the setup's round trip happened in your AC (suspect 7: rustyAC's camber at rest is -0.0527 / 0.0536 / -0.0428 / 0.0445) |
| **8. Pad speed** | at a standstill flick the stick from the centre to full lock, in AC and in rustyAC, recorder running | `python ac_telemetry.py --duration 5 --out re\scratch\laps\pad_ac.csv` (and `pad_rusty.csv`) | `steerAngle` against `t` | both must reach full lock in 11 physics steps (33 ms) |

After every real-AC run keep `Documents\Assetto Corsa\logs\log.txt` and `custom_shaders_patch.log`
(they are overwritten at the next start).

The order that costs least: **run 1** (your normal lap; it answers the traction control question at
once), **run 6** (two minutes), then **run 2**.

## 8. What was added

- `tools/corner_numbers.py`: the numbers of one stretch of a lap out of any telemetry CSV (speeds,
  g, loads, slip, temperatures, pressures, `tc`, force per load), and the pressure law's fit.
- `grip_probe` (`crates/rustyac-game/examples/grip_probe.rs`): the pace sweep of section 5.2.
  `cargo run --release -p rustyac-game --example grip_probe -- --sweep 1.0 1.6 0.02 [--set key=value]`;
  about 45 s per sweep. `--pace 1.2 --csv file.csv` records one pass, `--standing 1800` prints the
  car at rest. Other stretches: `--zone` / `--measure`.
- `rustyac.exe --pressure-law <a>,<b>`: NOT Assetto Corsa. Tyre pressure = `a + b x (core - 26)`,
  to try CSP's pressures (`10.97,0.084` for the F2004 at 14 C). Like `--air-density`, it is stored
  in a recording's header.
- In the physics, for experiments only (both off unless asked for): `ChassisEnvironment::
  experiment_tyre_pressure_law` and `experiment_mesh_on_ground`.

## 9. Open questions

1. Whether CSP's lower tyre pressure reaches the tyre's grip, stiffness and rolling drag through the
   game's own formulas or only shows in the telemetry. The standing car in your recording sits 1 mm
   lower than plain AC's (0.4 mm of it explained by rustyAC with CSP's law); a 22 % softer tyre
   alone would give more. Run 7 and run 2 tell.
2. Which CSP setting the pressure law is. No text file of the three CSP versions mentions it.
3. What CSP's "mesh collider sooner" rule is ("when car is not driving straight"), and whether it
   applies to a car without extended physics: CSP's own texts contradict each other. Run 6.
4. Whether CSP's thin air also reaches the engine (plain AC's engine uses the same density).
5. Why your last real F2004 log has no `Setup change for Car` lines.
6. No lap driven by you has been compared yet: every "real" number here is the AI or a standing car.
