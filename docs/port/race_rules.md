# Race rules: sessions, start, timing, flags, penalties, results (Task 28)

## Resume here

State at v0.28.0:

- **Done and proven**: the referee (`crates/rustyac-physics/src/race/`), in rustyac.exe
  (`crates/rustyac-game/src/referee.rs`), in the browser (`?sessions=`), mirrored online, with a
  text HUD. See "Results" for what was compared with the game and how.
- **To check the port again** (all read-only on the game):
  - `sh re/scratch/task28/micro.sh 25`: 200 made-up drives through the game's own referee and
    the port (`car_oracle race-micro`), about half a minute each.
  - `sh re/scratch/task28/final_a.sh; sh re/scratch/task28/final_b.sh`: the whole sessions
    (`car_oracle multi --scenario rc_*`), 10 to 40 minutes each. They need a copy of the oracle
    at `re/scratch/task28/bin/car_oracle_final.exe` (`tools/car_oracle/target/release/car_oracle.exe`).
  - `cargo test --release -p rustyac-physics --test race_rules`: the same without the game
    (hashes of approved runs).
- **Next (Task 28b)**: pit stops and the pit crew, the mandatory pit window's enforcement, AC
  replays, the ghost car. The hooks are listed under "Not done".
- Scratch files of this task: `re/scratch/task28/` (the three reading briefs `spec_*.md` say
  where every field of the game's classes is and are worth keeping).

## Summary in plain English

rustyAC now has a referee. Until this version a "race" was only cars on a grid and a green
light; now the program runs sessions the way Assetto Corsa does: practice, qualifying and race
one after the other, a clock or a lap count for each, the starting grid taken from qualifying,
the start lights with the gearbox held until just before the green, a penalty for moving too
early, penalties for cutting the track, the drive-through and the black flag, blue and yellow
flags, the chequered flag, positions during the race, the results table at the end, and the
results file `race_out.json` that Content Manager reads.

It is a translation of the game's own code (`RaceManager` and the classes around it), not a
re-invention. It was checked in two ways:

1. **The game's own referee, run inside the test program** on made-up cars, next to the port:
   every field after every frame, and the bytes of `race_out.json`. 201 drives over 8 session
   lists.
2. **Whole sessions with the game's real cars and AI drivers**: the game's code drives a
   3-lap race of 6 cars on Magione (and a timed race, a weekend, a jump start, cut laps with a
   black flag), the port replays the same inputs and has to end with the same physics, the
   same referee state every frame and the same `race_out.json`, with 0, 1 and 8 worker threads.

Both agree bit for bit (tables below). What is not there yet: pit stops, the pit window's
rule, the game's real HUD (the texts are the game's, the look is rustyAC's plain text).

## What is ported

All of it is in `crates/rustyac-physics/src/race/` unless said otherwise. No Windows code: the
same files run in the browser.

| Part | File | The game's functions (acs.exe) |
|---|---|---|
| Session list from race.ini | `manager.rs` (`SessionsIni`) | `RaceManager::initOffline` 0x14013a6c0 |
| Sessions, start, end, next, restart | `manager.rs` (`RaceManager`) | ctor 0x140132db0, `setCurrentSession` 0x140140df0, `startRace` 0x140141470, `update` 0x140141550, `isRaceOver` 0x14013eae0, `onLapCompleted` 0x14013f380, `finalizeCurrentSession` 0x140136b30, `skip` 0x140141350, `restart` 0x140140d00, `shutdown` 0x140141320, `resetInvalidStateCarsToPits` 0x140140340 |
| Positions during a race, the finish | `manager.rs` | `updateCarsRealTimePositions` 0x1401428b0, `updateCarsEndRace` 0x140142650 |
| A server's session as the referee's | `manager.rs` (`convert_remote_session`) | `convertRemoteSession` 0x140136a50 |
| Laps, sectors, bests, the leaderboard, `laps.ini` | `timing.rs` | `RaceTimingServices` ctor 0x140143c60, `onLapCompleted` 0x140145300, `onSectorSplit` 0x140145940, `updateLeaderboard` 0x140146660, `resetCurrentLaps` 0x140145d30, `shutdown` 0x140145f50, `LeaderboardEntry::operator<` 0x140144130 |
| Results of a session, AI cars that did not finish | `results.rs` | `SessionResultProcessor::getCompleteAIResults` 0x140166c70, `fetchBasicResults` 0x140166230, `processQualifyResults` 0x140167110, `processRaceResults` 0x140167f30, `getAverageLap` 0x140166730, `disqualifyCar` 0x140166100 |
| The grid from qualifying | `manager.rs` (`start_index`) | `CarRaceInfo::init` 0x1400e8070 and its session-end handler 0x1400e7a20, `getSpawnPositionIndex` 0x1400e8050 |
| Flags (which one is shown) | `flags.rs` | `FlagManager::update` 0x14009d140, `checkForBlue` 0x14009c390, `checkForYellow` 0x14009c550, `checkforPenalty` 0x14009c9b0, `setFlag` 0x14009cf00, `clearFlag` 0x14009cb60 |
| Penalties in the car | `car/chassis.rs` (`PenaltyManager`) | `PenaltyManager::step` 0x140265e10, `addJumpStartPenalty` 0x140265a90, `checkBlackFlag` 0x140265c10, `decreasePitPenaltyLaps` 0x140265c30, `reset` 0x140265d40, `resetPenalty` 0x140265d90; `Car::onNewSession` 0x140274c30 |
| Banner and message texts | `hud.rs` | `RaceManager::getRaceText` 0x140139150, `getPracticeQualifyText` 0x140138d80, `renderHUD` 0x14013f960 |
| `race_out.json` | `output.rs` | `JsonOutputFile` ctor 0x14010c0e0, `endSession` 0x14010d310, offline 0x14010d730, `writeOutputFile` 0x14010ecc0, and the game's `JSONWriter` |
| The glue to the cars | `referee.rs` | what `Sim` and `CarAvatar` do with the manager's events |

(`stepJumpStart` and `LapInvalidator` were already in the car from earlier tasks; this task
gave them the session and the penalty mode they wait for.)

**What the rules are, as the code has them** (worth knowing when driving):

- A session is practice (type 1), qualifying (2), race (3); hotlap (4) and drag (7) are read
  and run as a session without rules of their own. A race with `LAPS=0` is a timed race
  (`DURATION_MINUTES`); `[RACE] ADDITITIONAL_LAP=1` (the game's spelling) adds the lap after.
- Practice and qualifying end when the time is over. In qualifying a car on a lap may finish
  it: the session waits `max(10 s, best lap x OVER_TIME_MULT)` more (1.5 in the code, 1.2 in
  the game's shipped file).
- A race is over for a car when it crosses the line after the leader has finished. The
  session ends when every car is done or has stood still too long; then the results show.
- The start: the lights come `20 s` after the first session's start (`10 s` on later ones),
  plus a random -1..+1 s. Until `lockGearboxAtStartTimeMS` before the green the gearbox is
  locked. Whoever moves more than 0.1 m in that last window has jumped the start:
  `JUMP_START_PENALTY=0` just holds the car, `1` puts it back in the pits with the controls
  locked for 20 s, `2` is a drive-through (turned into mode 1 when the race is shorter than
  the laps the drive-through may wait).
- Cutting: a surface with `BLACK_FLAG_TIME` in the track's surfaces.ini. In practice and
  qualifying the lap is invalid (it does not count as a best). In a race a cut gives seconds
  to serve by slowing down; who has not served them at the line gets the black flag.
- A drive-through is served by 160 m in the pit lane. Three laps to do it; with one lap left
  and not served: black flag.
- The blue flag is shown to a lapped car with a faster car close behind; yellow when a car
  ahead on the track is slow or stopped. **The game has no white flag**: there is no code for
  one.

## How it fits together

The referee is a plain state machine. It never touches a car. Each "frame" it is given a view
of every car (laps, position round the lap, speed, in the pits or not) and the lap and sector
events of that frame, and it answers with a list of jobs: *lock car 3's controls*, *car 2 to
its pit box*, *new session*, *show the results*. The same code is fed by the desktop's physics
thread, by the browser, by the replay and by the oracle's stand-ins of the game's classes.

- **A frame is three physics steps** (9 ms). In the game the referee runs on the main thread,
  once a picture, whenever that is; a port that has to give the same bits on every PC and with
  any number of threads cannot depend on the frame rate, so the referee's clock is the
  physics'. The oracle drives the game's referee at the same rhythm.
- **Jobs that change the physics** (a new session's type, the start time, new wind) are done at
  the start of the next step, as the game's main thread hands them to its physics thread.
- **Random numbers**: the start's delay draws from the referee's own stream (the game's
  `rand()`, the same formula), seeded in the recording's header, so a replay draws the same.
- **A recording has what the referee needs**: the header line `referee=` (session list,
  penalties, jump-start mode, `OVER_TIME_MULT`, the two start delays, the seed) and two
  buttons among the step events (next session, restart). Nothing else: everything the referee
  decides follows from the physics.

## Results

Every comparison is bit for bit: no tolerance anywhere.

### 1. The game's own referee in-process (`car_oracle race-micro`)

The oracle builds the game's `RaceManager`, `RaceTimingServices`, `SessionResultProcessor`,
`CarRaceInfo`, `FlagManager` and `JsonOutputFile` inside its own process, on hand-made `Sim`
and `CarAvatar` blocks, and runs a made-up drive through them and through the port: cars with
lap times of their own that pit, stop, cut, get lapped, retire, jump the start; the player
presses "next session" and "restart" now and then. After every frame every field of both
sides is compared (session, leaderboard, lap database, best splits, real-time positions, the
flags shown, what was told to the cars), at the end the bytes of `race_out.json`.

| Session list (`--kind`) | What it is |
|---|---|
| 0 | practice, qualifying, race of 3 laps |
| 1 | timed race of 1 minute and the lap after |
| 2 | timed race of 1 minute |
| 3 | quick race of 5 laps |
| 4 | qualifying, race of 2 laps |
| 5 | practice without a time, timed race, practice |
| 6 | two qualifyings, two races |
| 7 | hotlap, practice, race |

| Session list | Drives | Identical | Cars | Frames | Laps | Values compared | race_out.json, bytes |
|---|---|---|---|---|---|---|---|
| 0 | 25 | 25 | 2 to 24 | 1,000,000 | 4,500 | 29,568,694 | 4,733 to 37,304 |
| 1 | 25 | 25 | 2 to 24 | 1,000,000 | 4,444 | 26,040,000 | 579 to 20,801 |
| 2 | 25 | 25 | 2 to 24 | 1,000,000 | 4,888 | 26,120,000 | 745 to 18,438 |
| 3 | 25 | 25 | 2 to 24 | 1,000,000 | 3,616 | 26,200,000 | 925 to 16,708 |
| 4 | 26 | 26 | 2 to 24 | 1,045,000 | 4,237 | 29,458,841 | 1,607 to 29,648 |
| 5 | 25 | 25 | 2 to 24 | 1,000,000 | 7,781 | 29,919,216 | 4,370 to 73,188 |
| 6 | 25 | 25 | 2 to 24 | 1,000,000 | 5,630 | 31,268,574 | 5,413 to 54,243 |
| 7 | 25 | 25 | 2 to 24 | 1,000,000 | 4,918 | 30,035,299 | 2,074 to 81,205 |
| **all** | **201** | **201** | | 8,045,000 | 40,014 | 228,610,624 | |

Also in the same runs: `convertRemoteSession` on 300 made-up server sessions per run (all the
same), and the flags. Three of the drives, as the oracle counts the frames each flag was on
the screen (the same on both sides):

| Drive | none | blue | black | chequered | yellow | penalty |
|---|---|---|---|---|---|---|
| seed 3, list 0, 9 cars | 26,760 | 0 | 0 | 1,487 | 4,303 | 7,450 |
| seed 4031, list 4, 16 cars | 13,595 | 0 | 2,860 | 6,417 | 4,790 | 12,338 |
| seed 1002, list 1, 12 cars | 8,971 | 455 | 0 | 5,046 | 2,421 | 23,107 |

(The blue flag to a lapped car with a faster one behind is proven here: it needs a car a lap
down close in front of a leader, which six equal AI cars over three laps do not give.)

### 2. Reading race.ini (`car_oracle race-ini`)

The game's own `initOffline` reads a race.ini, the port reads the same file; the session list
(type, name, laps, minutes, spawn set, forced position, timed or not) and the rules
(penalties, jump-start mode, extra lap, fixed setup) are compared.

14 files (`re/scratch/task28/ini/` and those of Task 24b): **IDENTICAL**, among them a
weekend, timed races with and without the extra lap, two races in one list, session types the
game does not know, and a gap in the `[SESSION_n]` numbers (the game stops at the gap, so does
the port).

### 3. Whole sessions (`car_oracle multi --scenario rc_*`)

The game's `RaceManager` and `RaceTimingServices` among the game's own cars, physics and AI
drivers (Task 27's harness, with the referee's jobs wired as `Sim` wires them). Every step's
row has the physics of every car and a hash over the referee's whole state. The port replays
the recorded inputs with 0, 1 and 8 worker threads and has to give the same rows and the same
`race_out.json`. Track: Magione; cars: BMW M3 E30 unless said otherwise.

| Scenario | Cars | Steps | Values compared | Workers 0 / 1 / 8 | race_out.json | The first car's penalties (referee frames) |
|---|---|---|---|---|---|---|
| rc_race | 6 | 112,000 | 158,704,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 3,745 bytes | none |
| rc_race | 4: Ferrari F2004, BMW M3 E30, Audi R8 Plus, Ferrari F40 | 60,000 | 52,140,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 2,469 bytes | none |
| rc_timed | 4 | 112,000 | 106,064,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 1,509 bytes | none |
| rc_timed_plain | 3 | 100,000 | 71,200,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 1,213 bytes | none |
| rc_weekend | 4 | 150,000 | 142,050,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 2,395 bytes | none |
| rc_weekend_q | 4 | 215,000 | 203,605,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 4,049 bytes | none |
| rc_jump | 4 | 40,000 | 34,480,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 2,106 bytes | 2682 locked after a jump start |
| rc_jump_dt | 3 | 150,000 | 94,050,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 2,449 bytes | 49062 with a drive-through pending, 11758 black-flagged |
| rc_cut | 3 | 100,000 | 62,700,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 1,548 bytes | 2142 with seconds to serve, 19280 black-flagged |
| rc_cut_practice | 2 | 66,000 | 25,872,000 | IDENTICAL / IDENTICAL / IDENTICAL | IDENTICAL, 611 bytes | 2 cuts in a lap |

What each one shows:

- **rc_race**: the task's first target: 3 laps, 6 AI drivers, start lights, the finish as each
  car crosses the line, the results, the file.
- **rc_timed / rc_timed_plain**: 2 minutes; with `ADDITITIONAL_LAP` the leader's lap after the
  clock is the last one, without it the lap in progress is.
- **rc_weekend / rc_weekend_q**: practice, qualifying, race, every session ending by its clock.
  In `rc_weekend` qualifying is one minute and nobody sets a time (the grid is then the entry
  list's order, as in the game). In `rc_weekend_q` it is four minutes and the quicker drivers
  sit in the later pit boxes: qualifying ends 3, 2, 1, 0 and the race starts from the grid
  3, 2, 1, 0 (the game's and the port's alike), not in the entry list's order.
- **rc_jump**: the first car is driven by hand and rolls in the window before the green:
  `JUMP_START_PENALTY=1`, back to the pits, controls locked for 20 s.
- **rc_jump_dt**: the same with `JUMP_START_PENALTY=2`: a drive-through that is never served.
- **rc_cut**: the first car leaves the track over the penalty surfaces beside Magione's line
  twice a lap in a race: seconds to serve, not served, the black flag at the line.
- **rc_cut_practice**: the same in practice: the laps are invalid.

### 4. rustyac.exe itself

The program itself, headless, an AI driver in the player's car (`--ai-drive`):

| Run | What happened |
|---|---|
| `--no-race-ini --ai-cars 5 --sessions r3` on Magione, 6 BMW M3 E30 | start lights, 3 laps, the results table (all 6 classified, 4:36.498 for the winner), `race_out.json` 3706 bytes and `laps.ini` written to `--out-dir` |
| a weekend from a race.ini as Content Manager writes one (`re/scratch/task28/ini/cm_weekend.ini`: practice 1 min, qualifying 1 min, race of 2 laps, 4 cars, `PENALTIES=1`, `JUMP_START_PENALTY=1`), recorded | the three sessions one after the other by themselves, the results table, `race_out.json` 2434 bytes, `laps.ini` 155 bytes |
| that recording replayed with 0, 1 and 8 worker threads | 1,038,788 steps each; state hash `2fb7d988db308b2e` three times; the two files' hashes (`41d28f6f2d682fa1`, `ec0a6df0b6b11a5c`) three times and the same as the live run's |

So a replay gives the referee's files again, bit for bit, whatever the number of threads.

### 5. Nothing changed for sessions without race rules

The executable of v0.27.0 (built from the tag in a throw-away worktree) against this one:

| Check | Result |
|---|---|
| 39 recorded drives without race rules (every 4th file of `oracle/game/*.ryin`: single cars, tracks, crashes, hybrids, drag strips), `--replay ... --dump-states -` | **39 of 39 byte-identical dumps** (up to 1.36 GB each) |
| an AI race recorded by v0.27.0 (4 cars, Magione, 114,621 steps), replayed by both | the same state hash (`4dee2fc31c2d4fc0`) |
| an AI practice recorded by v0.27.0 (3 cars, 151,692 steps), replayed by both | the same state hash (`ee76f7cca909f6b1`) |

A recording without a `referee=` line has no referee, so old recordings replay as before. (The
one change to the physics of a car without a referee is none: `PenaltyManager::step` runs for
the first car as in the game and does nothing while the penalty mode is off.)

### 6. Tests and builds

| Check | Result |
|---|---|
| `cargo test --release --workspace --locked` | 273 tests pass in 56 suites, none fails (among them the golden files of every earlier task: single car, several cars, AI, contacts, tracks, renderer, sound, multiplayer tapes) |
| `crates/rustyac-physics/tests/race_rules.rs` | 8 made-up drives against the hashes the oracle approved; a weekend runs through its sessions; a frame a microsecond longer gives another result (the comparison can fail); a race start with the game's referee replayed from a golden file with 0 and 3 workers |
| the multiplayer tapes of Tasks 25 and 26, run afresh on this code (`re/scratch/task26/proof.py --again`) | 20 tapes, **all four ways IDENTICAL** to the game at every step (desktop, the WebSocket transport, through a real gateway, the wasm client under wasmtime), with the digests of Task 26 |
| `cargo clippy --workspace --locked -- -D warnings` | clean |
| clippy for `wasm32-unknown-unknown` (rustyac-web, rustyac-net) and `wasm32-wasip1` (math, ode, content, physics, net, game) | clean: the referee has no Windows code |
| `cargo build --release --locked`, `python web/build.py` | pass (the site's wasm is 4.61 MB) |

## How to run a weekend

**From Content Manager** (or the game's own launcher): set up the weekend there (car, track,
opponents, practice / qualifying / race, penalties, jump start), press *Go* once so that it
writes `Documents\Assetto Corsa\cfg\race.ini`, leave the game, then:

```
rustyac.exe --car bmw_m3_e30 --track magione
```

rustyac.exe reads that race.ini by default: the sessions, the other cars with their AI level,
the conditions, `PENALTIES`, `JUMP_START_PENALTY`. (Car and track are still named on the
command line.) A file somewhere else: `--race-ini <file>`.

**Without a race.ini:**

```
rustyac.exe --car bmw_m3_e30 --track magione --no-race-ini --ai-cars 5 --sessions p10,q5,r3
rustyac.exe --car bmw_m3_e30 --track magione --no-race-ini --ai-cars 5 --sessions rt20+ --jump-start 2
rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --ai-cars 7 --ai-level 95 --sessions q10,r5 --penalties 1
```

`--sessions` is a list: `p10` practice of 10 minutes, `q5` qualifying of 5 minutes, `r3` a race
of 3 laps, `rt20` a race of 20 minutes, `rt20+` the same and the lap after. `--penalties 0|1`,
`--jump-start 0|1|2` as race.ini's keys. `--no-referee` gives the program as it was before
(cars on a track, no rules).

**While driving:** `F7` goes to the next session (as the game's "next session" button), `F8`
restarts the current one. The HUD's top shows the session and what is left of it, your
position and laps, the last sector, the flag in words, penalty messages; at the end of a
session the results table. The start lights are the game's.

**The results** are written when the program ends (and are complete after the last session):

- `race_out.json`: in `out\` beside rustyac.exe, or in the folder given with `--out-dir`.
  Never in the game's folder or in `Documents\Assetto Corsa`. The format is the game's, key
  for key, so Content Manager's result reader takes it; **to show it in Content Manager, copy
  it to `Documents\Assetto Corsa\out\race_out.json` yourself** (rustyAC does not write there).
- `laps.ini` beside it (the laps of the last session, as the game writes them).

**Unattended** (a check, or a recording to replay):

```
rustyac.exe --car bmw_m3_e30 --track magione --no-race-ini --ai-cars 3 --ai-drive --sessions p1,q1,r2 --headless --duration 420 --out-dir my_out --record weekend.ryin
rustyac.exe --replay weekend.ryin --headless --state-hash --physics-threads 8
```

`--next-session-at <s>` and `--restart-session-at <s>` press the two buttons at a time of the
run. The replay prints the hashes of `race_out.json` and `laps.ini` it would write.

**In the browser:** add `?sessions=p10,q5,r3` to the page's address (F7 / F8 as on the
desktop). **Online** (desktop and browser) the server is the referee; rustyAC mirrors its
session, the standings and the results in the same HUD lines.

## Not done

- **Pit stops and the pit crew, AC replays, the ghost car**: Task 28b, by the task's own
  wording.
- **The mandatory pit window**: the referee has the game's two fields
  (`RaceManager::pit_window_start_time` / `pit_window_end_time`, compared with the game's every
  frame) and that is the hook. Nothing sets them (they are 0, "no window") and nobody is
  disqualified for missing a window: the game decides that with the pit stop's code.
- **A black flag's "retire to the pits" teleport for the player** follows the game (the car is
  retired and stays where it is until the next session); the pit crew scene is 28b.
- **`laps.ini` was not compared with the game's writer.** The lap database it is written from
  is compared every frame; the file's text is written after the decompiled `shutdown`, but
  the oracle did not run that function (it writes into the game's own folder, which this task
  may not touch). `race_out.json` was compared because its writer takes a path.
- **The game's HUD** (the timing apps, the leaderboard app, flag pictures): the texts are the
  game's (`getRaceText`, `getPracticeQualifyText`, the penalty messages), the look is rustyAC's
  plain text lines.
- **Hotlap, drag race, drift and time attack have no rules of their own.** Their session
  types are read and run as a session with a clock; the points of drift and time attack and
  the drag tree are separate classes in the game.
- **Online the referee does not rule.** The server does. What the client mirrors: the
  session (through `convertRemoteSession`), the standings, the end of the race and the
  results. Penalties given by a server (`kick`, ballast, the server's own jump-start mode)
  are not shown beyond what Task 25 had.
- **New wind per session**: the game draws new wind at a session's start; rustyac.exe does so
  when the race.ini has a `[WIND]` section (without one there is no wind to draw). The whole
  sessions of the oracle have no wind, so this draw was not compared with the game there
  (the wind generator itself was, see `docs/port/conditions.md`).
- **A blue flag in a whole session with real cars** was not produced (see above); the flag's
  code is proven on the made-up drives.

## Open questions

Choices made without asking, to be confirmed or changed:

1. **The referee's frame is three physics steps.** The game's is a picture. Laps and sectors
   are timed by the physics either way; what the frame decides is only when (to 9 ms) a
   session's end or a lock is noticed. A different rhythm would be a different, equally valid
   run of the game.
2. **The start delay**: 20 s for the first session and 10 s for later ones, as `Sim::Sim` and
   `RaceManager` have it. `--race-start-in <s>` overrides it. The browser uses the same.
3. **`OVER_TIME_MULT`**: rustyac.exe reads `[RACE] OVER_TIME_MULT` of the game's
   `system/cfg/assetto_corsa.ini` (1.2 as shipped), as the game does; the browser has no such
   file and uses 1.2; the code's own default without the file is 1.5.
4. **The results table stays up until the session changes** (F7), as in the game; with
   `--headless` nothing waits for a key and the program ends by `--duration`.
5. **`race_out.json` names**: the driver's name is race.ini's `DRIVER_NAME` (or `--name`), the
   car's is the folder name, as the game writes them.
6. **Keys F7 / F8** for next session and restart were free; the game has no keys for them
   (they are buttons in its results screen and pause menu).
7. **One thing seen and not explained**: in an early variant of `rc_jump`, with a car standing
   still on the grid among AI cars, the second AI car left the game's values at step 4677.
   The final scenarios do not show it (they are identical throughout), and it is in Task 27's
   code (an AI driver passing a stopped car), not in the referee. Worth a scenario of its own.
8. **A bug of Task 27 found and fixed here**: an AI driver's qualifying tyre. A compound
   without a `[COMPOUND_n]` strategy counts as good for 100000 km in the game; the port had
   0, so in qualifying an AI driver could pick another tyre than the game's.
