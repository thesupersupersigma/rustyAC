# AC's launcher in the browser, keyboard steering (Task 37)

## Resume here

- Part 0 done (commit fd4fb66): the 20 multiplayer goldens made again under `re/gold/net/`
  (`tools/net_gold/`, see docs/port/multiplayer.md "Golden tapes"); one port bug found and
  fixed on the way (collision reports); 80 of 80 checks identical.
- Part A done: the keyboard's `stepSteer` is the game's, proven against the game in-process;
  speed sensitivity; the controls page; the launcher's cfg seeding.
- Part B.1: the pure-Rust ini copy (`crates/rustyac-launcher/src/profile.rs`) and the bridge
  building for wasm; proven against Windows and the desktop bridge. B.2 onward: see below.
- Notes from reading (not committed): `re/scratch/task37/launcher_split.md`,
  `theme_menu.md`, `web_now.md`.

## Part A: the keyboard's steering

### A.1 `KeyboardCarControl::stepSteer` (0x140111650), 1:1

Before this task the port's `step_steer` gave 0, so the keyboard's speed limit was always the
plain `π/(0.5·v+1)`. In the game, `stepSteer` looks ahead along the track's AI line
(`AISplineRecorder`'s best-lap spline, which `load` fills from `ai/fast_lane.ai`):

- with no line it puts the car on the nearest point once (`closestPointIndex`, 0 for an
  empty line) and the look-ahead point is the world's origin;
- with a line the index stays where the constructor put it (-1: the game never moves it),
  `getCarPositionOnSpline` projects the car on the segment there, and `getPointAtDistance`
  walks forward until `lookAhead` (8 m by default) is reached;
- the point in the car's own frame (`dBodyGetPosRelPoint`), `-x · steerGain`, not finite → 0,
  ±0.9, the understeer branch (never taken: nothing sets it), ±0.9.

`getKeyboardSteering` then keeps whichever is larger, that or `π/(0.5·v+1)`. The port
(`crates/rustyac-game/src/input/keyboard.rs`) follows the machine code's order of operations
(checked with `tools/disasm.py`, not the pseudo-C). Its car pose is the `CarProbe` the
keyboard already had (position and left axis of the body's world matrix); the line is the
track's, given to the live driver by `set_track` (desktop and browser).

One shortcut, commented in the code: the game's walk along the line never stops if the line is
shorter than the look-ahead; the port stops after one lap of points.

**Oracle.** `car_oracle steer-micro --seed <n> --root <folder>` builds the game's own objects
in-process: a `PhysicsEngine`'s rigid body (the game's ODE), a `KeyboardCarControl` with six
sets of settings (AC's first), and an `AISpline` loaded by the game's `AISpline::load` from a
copy of Laguna Seca's `fast_lane.ai` (2,331 points), or none. Every call puts the body
somewhere new (near the line, far from it, turned, rolled), with random speed, step, keys and
last steering, calls the game's `stepSteer` and `getKeyboardSteering`, and writes what went in
and came out (hex bits) to a tape. `keyboard_steering_check` (an example of `rustyac-game`)
plays a tape through the port.

| Tape | Calls | Workers 0 | Workers 1 | Workers 8 |
|---|---|---|---|---|
| seed 1, on the line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 1, no line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 2, on the line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 2, no line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 3, on the line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 3, no line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |

The line's points the game loaded are compared with the port's `AiSpline::load` too (all the
same). Fault test (`--fault`: `STEER_GAIN` one bit off): caught at call 1 on the line, at
call 32 without one.

Found on the way: the game's `steeringMovement` reads the car's own speed
(`Car::getSpeed`, +0x3e90), not the input's; in the game they are the same number, and the
port uses the input's for both. The oracle writes the speed into its stand-in car.

### A.2 Speed sensitivity (rustyAC's own setting, not part of vanilla AC)

`controls.ini` `[KEYBOARD] RUSTYAC_SPEED_SENSITIVITY` (0 to 1, default 1) scales the speed `v`
in both places the keyboard's steering uses it: the limit `π/(0.5·v+1)` and
`steeringMovement`. 1, or the key missing, is AC's own rule bit for bit (the tapes above,
played again with the key set to 1: IDENTICAL); 0 takes the speed out (a unit test: held
steering at 80 m/s reaches full lock with 0, about π/41 with 1).

The pad already has AC's own `[X360] SPEED_SENSITIVITY`; the controls page's pad slider
writes that key. There is no pad oracle in this task: that the port reads it as the game does
is from Task 26's port, not proven again here.

### A.4 The controls page

`crates/rustyac-launcher/src/controls.html`, rustyAC's own page (no Kunos files): the
keyboard's speed sensitivity (with an "Off" button: 0) and its three steering speeds, the
pad's speed sensitivity (with "Off"), steering speed and filter. It reads and saves the
launcher's `cfg\controls.ini` through the bridge's own `getiniasjson` / `savejsontoini`. On the
desktop the theme's controls button (`ac://showcontrols`) opens it in the launcher's window;
"Back" goes home to the theme. Checked with a launcher script (off screen): the page opened, a
sensitivity of 0.4 and the pad's Off were saved (`RUSTYAC_SPEED_SENSITIVITY=0.4` and
`[X360] SPEED_SENSITIVITY=0` in the file), Back returned to the theme.

### A.5 The launcher's cfg files seeded from the player's (rustyAC's own)

The launcher's player folder is rustyac.exe's, not `Documents\Assetto Corsa`. On a window
start (not `--transcript`, so the Task 35/36 comparisons are untouched), every `.ini` of the
player's real `Documents\Assetto Corsa\cfg` that the launcher's own `cfg` lacks is copied
first, then AC's own setup runs (and merges its defaults in, as always). The real folder is
only read. `--player-cfg <folder|none>` names another one.

Checked (`re/scratch/task37/seed.sh`, a fake "real" cfg folder): first start, `controls.ini`
and `video.ini` came from it, a `.txt` did not; second start, the launcher's own changed
`controls.ini` was kept; the real `Documents\Assetto Corsa` hashed the same before and after.
