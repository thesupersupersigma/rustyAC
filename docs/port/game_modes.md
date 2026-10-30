# Task 45: AC's game modes

## Resume here

- Steps 1 to 7 are done, one commit each (step 1 was the audit, no code). Released as v0.45.0.
- Left for Task 45b: see "Not done" at the end. Steam achievements and leaderboards are Task 47.

## In plain words

Hotlap, Time Attack, Drift and Drag Race now run as the game runs them, with the game's rules,
scores, messages, HUDs and end screens. Special events keep their gold / silver / bronze goals:
a hotlap, time attack, drift or drag event judges them as the game does, and an official race
event judges its race by finishing position at the AI level the player picked. The modes write
the same `race_out.json` the game writes, extras included (the launcher's Results page and the
career read those). In the browser, Drive's Time Attack, Drift and Drag Race are no longer
refused: they start from the theme like the other modes and run the same code.

The game's special-event files (`system/data/se/SPECIAL_EVENT_<n>.acd`) are decrypted in memory
only, as the game does. No decrypted copy is written anywhere (repository, `re/gold`, releases,
web pack); the proofs compare what the game reads from them.

## The audit, now

| Mode | Before | Now |
|---|---|---|
| Practice, Quick Race, Race Weekend | done and proven (Tasks 28, 30, 35, 36, 37c) | unchanged |
| Hotlap | done except special-event goals | goals judged like `HotlapMode` (oracle), results, page |
| Time Attack | gates drawn only (Task 22c) | budget, checkpoints, bonus time, penalties, goals, HUD, end screen, results, page |
| Drift | missing | `DriftModeComponent` scoring, `DriftMode` clock / levels / combos / goals, HUD, end screen, results, shared memory's drift fields, page |
| Drag Race | start lights only | tree, false starts, wrong-side disqualification, reaction time, matches, AI drag drivers, HUD, end screen, results, page (native proof; the test pack has no drag strip) |
| Track Day | not an AC mode | dropped (as agreed) |
| Special Events | launcher page and saves (Task 36) | plus the in-game goals of every kind, the official events' files read in memory |
| Career, Championships | launcher pages and saves (Task 36) | plus every `race_out.json` extra the theme scores from; there is no career code in acs.exe |

## Step 2: Hotlap and Time Attack

- `crates/rustyac-content/src/acd.rs`: `decrypt_simple` (`SimpleFileEncrypter::decryptFile`
  @ 0x14023b650) and `special_event(guid)` (key `ARUMBA_SPECIAL_EVENT_<n>`).
- `crates/rustyac-physics/src/data/ini.rs`: `IniReader::load_race` / `official_special_event`.
  As `RaceManager::initOffline` @ 0x14013a6c0, an official event's file replaces the whole of
  race.ini; every race.ini read of the desktop goes through it.
- `crates/rustyac-physics/src/race/game_mode.rs`: `GameMode` (the goals: BRONZE, SILVER, GOLD,
  GOLDEN GLORY, then MEDAL; ACHIEVED from the player's own race.ini) and the `Mode` the referee
  runs. One quirk kept: with no event file and GUID 0 the game's "NOT FOUND" text parses as GUID
  0, so there are no goals.
- `crates/rustyac-physics/src/race/time_attack.rs`: `TimeAttack::update` @ 0x1401be0b0 and its
  parts (budget `(estimate + 10) × BASE_TIME_MULT`, checkpoints from the lap's splits, points,
  lap bonus, the three penalties, goals). The sector estimate
  (`evaluateTimeFromTrackSpline` @ 0x1401bab80) is in `crates/rustyac-physics/src/ai/car_ext.rs`.
- HUD `crates/rustyac-gui/src/time_attack.rs`, end screen in `end_session.rs`; the gates now
  follow the real status (`crates/rustyac-game/src/render/ac.rs`).

## Step 3: Drift

- `crates/rustyac-physics/src/car/drift.rs`: `DriftModeComponent::step` @ 0x1402bff20 on the
  player's car every physics step (validity, extreme drifting, combos, banking).
- `crates/rustyac-physics/src/race/drift.rs`: `DriftMode` (`renderHUD` @ 0x14008b7c0 with
  `modeDrifting` @ 0x14008a1f0): 60 s clock, bonus seconds, level-ups, game over, goals.
- HUD `crates/rustyac-gui/src/drift.rs`; two tyres out allowed (`setAllowedTyresOut(2)`).

## Step 4: Drag Race

- `crates/rustyac-physics/src/race/drag.rs`: `DragMode` (`update` @ 0x140088c30,
  `onFinishLanePassed` @ 0x1400870e0, `computeCarTimes`, `isGameOver`, `processDragResults`
  @ 0x140166d70). The research brief had the Still and Suspended branches the wrong way round;
  the disassembly decided.
- The AI drivers are `AIDriverDrag` when the first session is a drag (`crates/rustyac-game/src/sim.rs`).
- The tree's custom start and its disqualified light reach the start lights
  (`crates/rustyac-render/src/start_lights.rs`).
- HUD `crates/rustyac-gui/src/drag.rs`. Found on the way: Rust's `{:.1}` rounds 6.25 to 6.2,
  the game's printf to 6.3; the mode HUDs use `widgets2::format_fixed`.

## Step 5: Special events in the game

- `RaceEvent` (official race events only, as `Sim::Sim`): the goal tier is the last goal whose
  objective is at most `[RACE] AI_LEVEL`; a win with no black flag gives that tier and every one
  below it. `initOffline` reads `AI_LEVEL` from the player's race.ini before the swap, so the
  port keeps the player's level in the official file.

## Step 6: Career and championships in the game

acs.exe has no career or championship code: the theme scores from `race_out.json`
(`career2.js` `bypoints` / `bytime`, `results.js`). What the game owes them is the extras:
`timeattack {points}`, `drift {points, combos, levels}`, `drag {total, wins, runs,
reaction_time, max_speed}`, `special_event {guid, max, tier}` and `bestlap {time}`. The drag
block is in the file (the research brief said it was not; the disassembly of
`addExtraSessionInfo` @ 0x14010c5e0 says it is).

## Step 7: the page

- `web/theme_glue.js` no longer refuses Time Attack, Drift and Drag Race; `web/launcher.js` no
  longer turns a saved `[SAVED] DRIVE` of those back into Practice.
- `crates/rustyac-web/src/race.rs` builds the mode's goals, the Time Attack multiplier and the
  drift's two tyres out from the launcher's race.ini, as the desktop does.

## How to start each mode

- Desktop: from the launcher (Drive, pick the mode, Start), or `rustyac.exe --race-ini <file>`
  with `[SESSION_0] TYPE=4` (hotlap), 5 (time attack), 6 (drift), 7 (drag, `MATCHES=n`, two
  cars), for example `rustyac.exe --race-ini race.ini --track ks_drag --layout drag400 --car
  bmw_m3_e30`. `--sessions` only knows p, q and r.
- Page: Drive, pick the mode, Start (Full mode).

## Proofs

| What | How | Result |
|---|---|---|
| Event files | the game's `decryptFile` on all 209 `SPECIAL_EVENT_*.acd` against the port, in memory | identical (360,305 bytes) |
| Goals | the game's `GameMode` constructor on 212 race.ini files (every official event, mods, broken ones) | identical (705 goals) |
| Hotlap | the game's lap handler on 8,480 made-up laps a seed | identical (400 messages) |
| Time Attack | the game's `TimeAttack::update`, every field every frame: 24 sessions, 25,860 frames, 57 checkpoints (seed 12; seed 7 likewise) | identical |
| Sector estimate | the game's `evaluateTimeFromTrackSpline` at Spa, Magione, Imola | identical |
| Drift scoring | the game's `DriftModeComponent::step`, every member every step: 80,000 steps, 2,347 drifts (seed 12), 3,260 (seed 7) | identical |
| Drag | the game's `DragMode`, every field every frame: 5 sessions, 19,886 frames, 10 matches, 2 penalties (seed 12; seed 7 likewise); AI drag drive (`car_oracle multi --scenario ai_drag`) | identical |
| Race events | the game's `RaceEvent` on 40 race.ini files, 800 race ends | identical |
| race_out.json | the game's writer with each mode's block among its objects, whole file, seeds 7 and 12 | identical for time attack, drift and drag |
| HUDs | render oracle `ta_hud`, `ta_none`, `drift_hud`, `drag_hud`: command log, pixels, state | identical |
| Page | `tools/proofs/ai_page.sh`: the desktop's drive (an empty Documents folder, as the page's), then native `race_proof` and the page in Chrome replaying its tape: Time Attack at Spa (2,117 points), Drift at Spa (2 combos), Drag at ks_drag drag400 (two e30s, 3 matches; native only) | race_out.json identical, 5 of 5 |

The quick run of every area (`run_all.sh --quick`, 156 cases, 56 min) is identical too. Every
oracle was also run with one rule of the port broken on purpose, and caught it. The
oracles run in `car_oracle mode-micro` (`--kind 5|6|7` for the extras) and
`tools/proofs/fresh.sh` part `modes`; the HUDs are in `tools/proofs/gui_list.txt`.

## Where the saves go

The game modes add no save file of their own. `race_out.json` goes where it always went
(Documents\Assetto Corsa\out, or `--out-dir`). The theme writes the rest from it, as proven in
Task 36: personal bests (`personalbest.ini`), special-event tiers (the theme's own store,
`__AC.setDbValue("se_tier_<event>")` in `results.js`, in the launcher's browser storage), career
(`launcherdata\filestore\career.ini`), championships (`champs\<guid>.champ`, `champs.ini`).

## Not done (Task 45b)

- `overlays.ini` creation: the game writes one for a layout without it; rustyAC never writes
  into the install. All Kunos layouts ship it; 9 mod layouts on this PC lack it (no gates drawn
  there, the checkpoints still work).
- Time Attack and Drift run on the referee's 9 ms frame instead of every rendered frame
  (`shortcut:` comments); the game's per-frame timing differs by up to a frame.
- The tyre smoke threshold of drift (`tyre_smoke.ini SLIP_LEVEL`) is the shipped 0.8, not read.
- Drag finish speed in mph (`Speed::useMPH`) is not followed: km/h always.
- The page's Special Events, Career and Championships tiles stay off; the page does not read
  official event files yet.
- A drag in the page is proven natively only (the test pack has no drag strip).
- No windowed desktop run of the new modes was looked at by a person.

## Open questions

- Should the page's test pack gain `ks_drag` so the drag can be proven in Chrome too?
