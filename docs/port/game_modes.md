# Task 45: AC's game modes

## Resume here

- Task 45 (v0.45.0), Task 45b (v0.45.1) and Task 45c (v0.45.2) are done; Task 45c's leftovers
  (for Task 45d) are under "Not done" at the end. Steam achievements and leaderboards are Task 47.

## Task 45c: the modes' clocks and the page's event tiles (v0.45.2)

In plain words: Drag Race now runs on the drawn frame like Time Attack and Drift, and Time
Attack's lap and split events reach it on that frame. The page's Special Events read an official
event's own file (decrypted in memory only), so its goals and the tier it saves are the
desktop's; the page's Career tile works; Championships matched already. Two leftovers from the
M1 proofs are fixed. The drift HUD question of Task 45b needed no change: the game never stops
drawing the HUD on a monitor.

| Step | What | Proof | Result |
|---|---|---|---|
| 0a | M1: Vallelunga extended `ai_solo` (`--ai-line`) differed since v0.45.0. Cause: the case swaps the AI line after the track loads (`Track::replace_ai_line`), and the port kept the sector positions measured on the old line; the game adds its timing lines after the AI line. `replace_ai_line` now measures them again (`timing::init_time_lines`) | `car_oracle multi ai_solo ks_vallelunga --layout extended_circuit --ai-line re/gold/fresh/vallelunga_extended_v6.ai` | threads 0/1/8 identical, 36,000 steps (estimate was `420f8239,422cb15f,4202b7f8` vs the game's `420f7d2d,422cfca0,420271c3`). It is the only case with `--ai-line`, so no other layout's result changes |
| 0b | M1: the browser case `conditions_spa_setup_launch` names the deleted `re\scratch\task15\god_tc4.ini`. `browser.sh` remaps that folder to `re/gold/fresh` (`replay_hash --remap`; `oracle/` stays read-only) | the case, desktop and wasm | 8,001 steps, hash `7d5ec97f570b8a4a` both: identical |
| 1 | Drag Race on the drawn frame (`DragMode::update(dt)` is a per-frame game object, like Time Attack): fades and `update` take the frame's dt; `mode_frames.txt` gains `dg_*` lines | `rustyac.exe --headless --drag-drive --drawn-frames 45` → `car_oracle mode-micro --mode-frames` (the game's `DragMode` with the same frames and events); fresh area, part `modes` | 5,339 frames, 7 events (2 matches, 2 messages) identical; a changed dt (frame 2,500) and a changed finish speed caught. Fresh `modes`: 11 of 11 identical |
| 2 | Time Attack's lap and split events handed over on the drawn frame (`CarAvatar::updateFromChannels` in the main frame), not at the physics step | Spa drive, `ta_ev` lines in `mode_frames.txt` checked against the game's `TimeAttack::update` | 9,023 frames, 4 checkpoints identical; events removed: caught at frame 1,720 |
| 3 | Drift HUD with the GUI hidden: `Game::onIdle` calls `renderHUD` only while `isRenderingGui` (Game+0x8). A sweep of every byte store to it finds two, both 0, in `OculusManager` and `ViveManager` (VR draws the HUD elsewhere). Hiding apps (H) never touches it, so on a monitor the drift clock never stops: rustyAC already does that | binary sweep (`re/scratch/task45c/scan_gui.py`) | no change needed |
| 4 | Page Special Events: an official event (`[SPECIAL_EVENT] GUID`) reads `system/data/se/SPECIAL_EVENT_<n>.acd` in place of race.ini, as `conditions.rs`; the page loads that file for the drive and the folder walk lists it | SPECIAL_EVENT_7 (drag, 12 matches, goals 3/5/6 wins) driven by the AI drag driver: desktop recording replayed by the page's code natively and in Chrome; launcher flow desktop vs page (`tools/web_checks/task45c/special.txt`) | 199,380 steps, chain `29b35684119d935d`, race_out.json `e087b6a4db835c2e` (tier 2) the same natively and in Chrome (52 fps); race.ini's goals changed to 50: the same (the goals are the event file's). Launcher: 262 = 262 requests, goals `3:0,5:0,6:0`, saved `se_tier_special_event_7=2`, shown `2 1,1,1` on both |
| 5 | Page Career tile (was hidden): the folder walk lists `system/locales/launcher` (the career names its series from `career_<language>.ini`; without it every series threw) and the career's videos (read only when played); the page's message box keeps the theme's overlay (it wiped the first run's language picker, so the page never asked the language). Championships needed nothing | `tools/proofs/launcher/career.txt` and `champs.txt` (with a MARK), desktop vs page from the same player folder | Career 109 = 109 requests, in order; Championships 203 = 203 (same set; 22 lines in another order: the theme's timers), the `.champ` file the same but its random GUID |
| 6 | `?dragdrive=1`: rustyac.exe's `--drag-drive` test mode in the page (`race_proof --drag-drive` too) | used by step 4 | as step 4 |

`run_all.sh --quick` was stopped by the machine running low on memory after six areas, all
0 different: physics 9/9, net 16/16, browser 9/9, fresh 54/54, sound 4/4, render 14/14. The
gui, python, replays, ai_page, shaders, launcher and dumps areas did not run.

The launcher checks start both sides from the AC folder's `cfg/` (the page takes the picked
folder's `cfg/` as the player's; the desktop check copies it into its `--docs`). Requests are
compared URL-decoded, the player's name, the theme's random road temperature, the clock's
`lastselected` and new GUIDs blanked (`tools/web_checks/task45c/cmp45.py`).

## Task 45b: the modes' leftovers (v0.45.1)

In plain words: Time Attack and Drift now update once per drawn frame with that frame's time, as
the game does, on desktop and in the page. A scripted drift drive on the drift track scores, and
every step and frame of it matches the game's code. Drift reads its tyre-smoke threshold from
`tyre_smoke.ini`, the drag finish speed follows the player's mph setting, layouts without
`overlays.ini` get their Time Attack gates as the game makes them, and Drag Race is replayed in
Chrome too.

| Step | What | Proof | Result |
|---|---|---|---|
| 1 | `Referee::drawn_frame(dt)`: Time Attack, Drift and the results fade run once per drawn frame (`Game::onIdle`: `update(deltaT)` then `renderHUD(deltaT)`, `GameTime` clamps 0.2 s). Desktop window: the display pushes its `GameTime` dt to the physics thread; page: `Web::frame` after `Session::advance` with the counted seconds. Headless, the picture path and tape replays (frameProof, `ai_page.sh`) keep the 9 ms frame | `rustyac.exe --headless --drawn-frames <seed>`: drawn frames of 5 to 40 ms between the physics steps, each written to `mode_frames.txt`; `car_oracle mode-micro --mode-frames` feeds them to the game's `TimeAttack::update`, `render_oracle gui --gui drift_drive` to `DriftMode::renderHUD` | Time Attack at Spa: 9,023 frames, 4 checkpoints, identical every frame (one dt changed: caught at frame 2,999). Drift: 1,165 frames, command log, pixels and state identical |
| 2 | `--drift-drive`: a scripted driver (`autodrive::DriftDriver`) on the drift track in `bmw_m3_e92_drift`, aids off, power slides with counter-steer | every physics step's `DriftModeComponent` inputs (`ds` lines) through the game's `DriftModeComponent::step`; frames through `DriftMode::renderHUD` with the event's goals | 8,794 steps, 2 drifts, 1 combo, 78.49 points, BRONZE and GOLD achieved: identical every step and frame (a lateral-speed change mid-slide caught at step 4,373) |
| 3 | `tyre_smoke.ini [TRIGGERS] SLIP_LEVEL` read as `DriftModeComponent::init` reads it (0 when missing), desktop and page; input files keep it when not 0.8 | mode-micro writes the game's static `triggerSlipLevel` (0x14151d940) per part: 0.8, 0, 0.5, 1.2 | seeds 7 and 12 identical (1,297 and 2,604 drifts); pinned 0.8 caught at step 0 |
| 4 | Drag finish speed in mph with `gameplay.ini [OPTIONS] USE_MPH` (rustyAC's cfg, then Documents; the page's own cfg, then Documents); `--drag-drive`: the game's test mode, the AI drag driver drives your car | `DragMode::onFinishLanePassed` with both settings was already in the drag oracle; live drives | USE_MPH=1: max speed 95, =0: 154 (km/h) |
| 5 | A layout without `data/overlays.ini`: the gates made **in memory** as `TimeAttack::createAndSaveOverlays` writes them (rustyAC never writes into the install, nor a copy into its cfg) | `render.sh` "overlays": the game's own function writes into a scratch folder, compared byte for byte, then the gate view drawn by both | 14 of 14 layouts identical (Bahrain ×6, ct_backwoods, monaco_2017, vhe_interlagos ×2: 1 or 3 gates; ks_drag ×5: no `AC_TIME_` nodes, nothing written, as the game). ks_brands_hatch has no AI line and is skipped |
| 6 | The test pack (`tools/proofs/test_pack.toml`, 2,064 MB) gains `ks_drag/drag400` and `bmw_m3_e30` | `ai_page.sh` replays the drag tape in Chrome | 9 of 9 identical, drag included |
| 7 | Hotlap, Time Attack, Drift and Drag in a normal window (`--no-focus`, scripted drives) | screenshots (≤ 300 KB) | HUDs and end screens drawn; two fixes: `--drift-drive` in a window, and a car an AI drag driver drives was run unpaced |

Step 8 (the page's Special Events, Career and Championships tiles) was not started.


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
| Drag Race | start lights only | tree, false starts, wrong-side disqualification, reaction time, matches, AI drag drivers, HUD, end screen, results, page (in Chrome since Task 45b) |
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
| Page | `tools/proofs/ai_page.sh`: the desktop's drive (an empty Documents folder, as the page's), then native `race_proof` and the page in Chrome replaying its tape: Time Attack at Spa (2,117 points), Drift at Spa (2 combos), Drag at ks_drag drag400 (two e30s, 3 matches; in Chrome since Task 45b) | race_out.json identical, 9 of 9 (Task 45b) |

The quick run of every area (`run_all.sh --quick`, 156 cases, 56 min) is identical too (Task 45b: 161 cases, 1 h 00 min, all identical). Every
oracle was also run with one rule of the port broken on purpose, and caught it. The
oracles run in `car_oracle mode-micro` (`--kind 5|6|7` for the extras) and
`tools/proofs/fresh.sh` part `modes`; the HUDs are in `tools/proofs/gui_list.txt`.

## Where the saves go

The game modes add no save file of their own. `race_out.json` goes where it always went
(Documents\Assetto Corsa\out, or `--out-dir`). The theme writes the rest from it, as proven in
Task 36: personal bests (`personalbest.ini`), special-event tiers (the theme's own store,
`__AC.setDbValue("se_tier_<event>")` in `results.js`, in the launcher's browser storage), career
(`launcherdata\filestore\career.ini`), championships (`champs\<guid>.champ`, `champs.ini`).

## Not done (after Task 45c, for Task 45d)

- The preview pack has no event tiles' content until it is made again: `web_pack` now takes
  `content/career/*.ini` and `system/data/se/*.acd`, but not the career's and events' pictures
  and videos (617 MB and 144 MB). With a folder of one's own everything is there.
- Only a drag event was driven to a tier in the page: the autodriver does not reach the Time
  Attack events' points (SPECIAL_EVENT_35: 4,441 of 6,000), so a TA or hotlap event's tier is
  proven only through the shared code.
- `tools/proofs/dumps.sh` still leaves out the setup drive (`conditions_spa_setup_launch`): its old
  rustyac.exe has no `--remap`.
- The drift drive's proof runs headless at an uneven drawn-frame rate; the windowed drawn frames
  were only looked at (screenshots), not compared.

## Open questions

- The page takes the picked AC folder's `cfg/` as the player's starting files, while the desktop
  launcher starts from an empty Documents folder; the theme then sets other camera and AI
  defaults (g-force 1.4 against 1.0, AI level 98 against 100). Which start should the page copy?
