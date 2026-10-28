# Task 43: player-facing leftovers, and pack updates by delta

## Resume here

- Done: steps 1 to 7 (step 3's live stop and step 5's live ESC run not shown: Task 43b).
- Next: the proofs (`run_all.sh --quick`), the web build, the release.
- Scratch: `re/scratch/task43/`. Rerun scripts: `tools/web_checks/task43/`.
- Disk at the start: 67 GB free.

## Step 1: `web_pack --delta <server manifest>`

The pack on the server is about 28.6 GB, too big for a second copy on this PC. `--delta` builds the
same pack as before, but it only writes files the live pack doesn't have yet. A file counts as
unchanged when it has the same path, size and SHA-256 as in the live `manifest.json`; such a file
is read for its hash but not written. The new full `manifest.json` goes with the changed files.
The tool prints how many files changed and how big they are, and which files of the live pack the
new pack no longer has. `--out` must be an empty folder, so it holds exactly the upload.

How to use it (the pack on the server under `/srv/rustyac/preview`, for example):

```sh
scp me@server:/srv/rustyac/preview/manifest.json live_manifest.json
cd tools/web_pack && cargo run --release -- --out ../../dist-web/delta --delta ../../live_manifest.json
# prints: delta: N new or changed files, X MB (plus manifest.json) in ../../dist-web/delta ...
#         the new pack no longer has M files of the live pack (...): delete them there
#           gone: content/tracks/...
scp -r ../../dist-web/delta/. me@server:/srv/rustyac/preview/
# then delete the "gone:" files on the server (optional: the page never asks for them)
```

`--config`, `--ac` and `--dry-run` work with `--delta` as without it (`--dry-run` writes nothing).

Proof (`sh tools/web_checks/task43/s1/delta.sh`, output in `re/scratch/task43/s1`): an old pack
(F2004 at Laguna Seca and Magione) and a new one (F2004 and Z4 at Laguna Seca: a car more, a track
less). One file of the old pack (`content/cars/ks_ferrari_f2004/sfx/ks_ferrari_f2004.bank`) is
changed, both its bytes and its hash in the manifest, so the delta has to carry it again. Then:
old pack + delta copied over it − the "gone" files = the server's update. That result is compared
with `diff -r` against the full new pack.

| Pack | Files | Size |
|---|---|---|
| old (full) | 1871 + manifest | 1451.4 MB |
| new (full) | 1926 + manifest | 1234.4 MB |
| delta | 100 + manifest | 179.5 MB |
| gone (to delete on the server) | 44 | 319.8 MB |
| old + delta − gone vs new, `diff -r` | 1927 vs 1927 | **identical** (the changed `.bank` is in the delta) |

Test: `a_manifest_reads_back` (`tools/web_pack`): a manifest written by `Manifest::to_json`, with
`"`, `\` and a control character in its paths, reads back with every path, size and hash.

## Step 2: `race_out.json` after an online session

rustyac.exe now writes `race_out.json` when it leaves a server, as acs.exe does: the same file
the offline referee writes, made the way `RaceManager` makes it online. What the game keeps of
a session online is `ACClient`'s: every car's laps from the server's lap messages
(`onRemoteLapCompleted` @ 0x14004f470: the time, the cuts, the car's compound; the player's own
laps too, as the server counts them), the sector times (`onSectorSplit` @ 0x14004ff70 for the
player's car, `onRemoteSectorSplit` @ 0x14004fdc0 for the others; `storeSplitToLap` makes the
last sector the lap less the others) and the server's standings (`sessionResultsGT`). From
that `RaceManager` makes a session's results in two places, both ported:

- when the server goes on from a practice or qualifying (`lambda_d00cd7` → `lambda_9cebb0`
  @ 0x140133f90, before `onNewSession` clears the laps; after a race it makes none),
- when the program ends (`RaceManager::shutdown` @ 0x140141320, the online branch), whatever
  the session.

Then `JsonOutputFile::onlineEndSession` (already ported in Task 28) writes them: the session's
place is the first of the server's sessions of its type (`ACClient::getCurrentSessionIndex`),
the lap counts are the server's (`RaceManager::getLapCount`), the player's own count and best
lap come from its car (`physicsState`). There is no `laps.ini` online (no
`RaceTimingServices`), as in the game.

Code: `crates/rustyac-net/src/laps.rs` (the lap lists, the results; a unit test),
`OnlineSession::start_results` / `end_session` / `finish` in `crates/rustyac-game/src/online.rs`,
`referee::finish` falls back to it. Two fixes on the way, both as the game does it:

- The pit box after an online session's results (Task 42b) reset the car's lap timer and lap
  count. The game's `CarAvatar::goToSpawnPosition` @ 0x1400d3ae0 only moves the car; only
  `CarAvatar::onNewSession` @ 0x1400d91c0 resets the timer. The player's lap count read 0 in
  every session before this.
- A run that ended because the server went away wrote nothing. `RaceManager::shutdown` runs
  however the game ends, so online the file is now written then too.

The browser page: `Game::finish_race` (`crates/rustyac-web/src/web.rs`) now gives the online
`race_out.json` too, and `returnToLauncher` in `web/app.js` writes it for an online drive as
well, so AC's launcher in the page shows it on its results page.

Live check (`sh tools/web_checks/task43/s2/online_out.sh 600`, then
`py -I tools/web_checks/task43/s2/compare.py re/scratch/task43/s2/run`): Kunos' acServer on
127.0.0.1, Laguna Seca, practice 2 min, qualifying 2 min, a 1-lap race; rustyac.exe "a" and
"b" (F2004, autodrive) through all three. Each client's `race_out.json` against acServer's own
results files (`results/*_PRACTICE.json`, `_QUALIFY`, `_RACE`):

| session | laps (driver, time, sectors, cuts) | best laps | race order | a's file | b's file |
|---|---|---|---|---|---|
| Practice | 4 | a 1:18.111, b 1:18.097 | | = acServer | = acServer |
| Qualify | 2 (b did none) | a 1:18.040 | | = acServer | = acServer |
| Race | 1 (b did not finish the lap) | | a, b | = acServer | = acServer |

Lap counts in the files: a 2 / 2 / 1 laps, b 2 / 0 / 0, the server's. Each file lists 12 cars
(the entry list's slots; the empty ones without a name) with the client's own car first, as
`Sim::getCar` orders them. An earlier run, before the pit box fix, had every lap and sector the
same but the player's own count 0; one where the server stopped under the clients wrote no
file (before the second fix).

Not proven: the page. No script joins a server through AC's launcher in the page any more
(Task 37b's flows were scratch and are gone), so the results page after an online drive in the
browser was not run. The page calls the same Rust code as the desktop run above. Other cars'
tyre compounds are the ones they last told the server of (their `tyres.ini` default before),
not read from their car as the game reads them (`shortcut:` in `online.rs`).

## Step 3: the old mouse pit menu

The game gives every keyboard driver the old mouse pit menu (`Sim::Sim` @ 0x140192070:
`USE_MOUSE_PITSTOP` in `pitstop.ini`, or controls that are the keyboard's), and then no pit
strategy page. Task 34 had ported its logic (`MousePitstop`, proven by the car oracle's
`pit_mouse`) but drew none of it. Now rustyac.exe draws it and hears it:

- `PitStopHud::new` (`crates/rustyac-gui/src/pit_menu.rs`) makes the controls `PitStop::PitStop`
  @ 0x1400a55d0 makes, in its order: the parent `PIT_STOP_PARENT_CONTROL` (650 x 600, in no GUI
  tree), "NO CHANGE" and a button per compound, the fuel buttons and the gauge (a new widget,
  `PitStopFuel`, `crates/rustyac-gui/src/widgets3.rs`), the three repairs and CONFIRM.
- `render_hud` draws the state "choosing" as `PitStop::renderHUD` @ 0x1400aa300 does: the three
  titled panels, the tank's line, the estimate, "CONFIRM", the controls.
- The window's mouse goes to the menu as the stop's three handlers pass it on; the clicks go to
  the physics thread (`GameSim::pit_click`, `Referee::mouse_pit_click`).
- A keyboard driver (`INPUT_METHOD=KEYBOARD` in `controls.ini`) or `USE_MOUSE_PITSTOP` gets it;
  the header of a recording says so (`pit_menu_mouse 1`), and the clicks are kept in
  `<input>.pit` (step 5). `--pit-click <control>@<s>` clicks it in unattended runs.

The menu starts visible, as a ksgui `Control` does (Task 34's port and the car oracle's stand-in
had it hidden until `PitStop::update` showed it; both changed, the car oracle's `pit_mouse` run
again).

| Proof | Result |
|---|---|
| render oracle `pit_stop` (now with the game's own `PitStop` constructor, stand-ins for the pit marker's model, the spawn position, the skin and the pit crew) | 30 frames **IDENTICAL** (command log, pixels, state) |
| render oracle `pit_mouse` (new): the menu opened in the box, every compound, "NO CHANGE", fuel + / - / fill, the gauge past and before the tank, the three repairs, clicks beside the controls and outside the panel, the estimate and the tank changing, the pause and ESC menus, the race over, another state | 74 frames, 49,363 calls: command log, pixels, state and events **IDENTICAL** (35 click events) |
| the same with "BUT_FUEL_UP" one pixel to the right (a fault put in on purpose) | **DIFFERENT** from frame 1 (66 frames of the command log, 73 of pixels) |
| car oracle `pit_mouse`, seeds 1 and 4, 0 / 1 / 8 workers | **IDENTICAL**, `race_out.json` identical (hashes `d5995828dcb84eaa`, `f060c67406fc308a`) |
| rustyac.exe live (`sh tools/web_checks/task43/s3/live.sh`): a keyboard driver's `controls.ini`, `--ai-drive` called in, the menu clicked every 2 s, recorded, replayed with 0 / 1 / 8 workers | the clicks are in `a.ryin.pit` and the three replays give one state hash (`cfc68622714f8928`, 197,575 steps); **no stop was made**: in a race of 3 laps the AI driver drops a call in its last two laps, and in a practice it did not leave the box. A live stop through the mouse menu is not shown (Task 43b) |

CONFIRM is not clicked in the render oracle: the game's handler asks the car for the stop's time
(`CarAvatar::getPitstopTime`); the car oracle's `pit_mouse` proves that part.

Not done (Task 43b): the page's keyboard drivers still get the quick menu (`crates/rustyac-web/src/race.rs`);
online the mouse menu is not given (the online pit stop is `OnlinePit`'s); `[PIT_INDICATOR]
ONLY_IN_PITLINE` (the pit marker shown only in the pit lane) is not ported; rustyAC's own
drivers (`--autodrive`, `--ai-drive`) keep the quick menu unless `--pit-click` is given, as they
click no CONFIRM.

## Step 4: votes and chat on the wire

`car_oracle net <tape>` now also hands every chat line and vote rustyAC sent on the tape to the
game's own `ACClient::sendChat` @ 0x140052a90 / `sendVote` @ 0x140052ba0 (on the in-process
`ACClient`, with the stand-in sockets) and compares what the game sends, byte for byte, with what
rustyAC sent (`NetGame::send_as_game` in `tools/car_oracle/src/game/net.rs`). A kick vote's target
goes in as the target car's avatar guid, as the game's console and the vote window give it, and
the game's `getSessionIdFromCarAvatarID` turns it back into a slot.

The tape: a live session on 127.0.0.1 (acServer, Laguna Seca), rustyac.exe "a" taped and "b";
a says three lines (short, long, not ASCII) and votes seven times
(`sh tools/web_checks/task43/s4/votes.sh`):

| message | count | game vs rustyAC |
|---|---|---|
| chat `0x47` (`hi`, a 36-character line, `ciao, è già`) | 3 | identical |
| vote next session `0x64` (yes, no) | 2 | identical |
| vote restart `0x65` (no, yes) | 2 | identical |
| vote kick `0x66` (a driver not there: target 0xff; b: slot 1) | 2 | identical |

acServer took them: its log has the three lines, "Creating vote to NEXT SESSION from a", the
answers, and "ACP_VOTE_KICK_USER failed, cannot find car with session id= 255" for the kick of
nobody. The rest of the same run (200 steps, net_watch) stayed identical. The three goldens' tapes
(`rich_*`) carry one chat line each, which the quick proofs' net runs now also compare.

## Step 5: Task 34's leftovers

### A car with no `setup.ini`

Task 34 wrote that the game makes `PRESETS_COUNT` preset buttons and empty `[PRESET_n]` sections
when the car's `setup.ini` is not there, and the port none. Read again, the difference was
elsewhere: both make the buttons and the empty sections, but `QuickMenuPitstop::buildControls`
@ 0x1400ae7b0 adds every item (fuel, compound, pressures, wings and the three repairs) only
inside `if (setupIni.ready)`, and the port added fuel, compound and the repairs always. The port
now does as the game (`QuickMenu::build` in `crates/rustyac-physics/src/race/pitstop.rs`).

The new sequence `setup_pitbare@<car>` is `setup_pit@<car>` with the car's `setup.ini` taken out
of the root (`tools/render_oracle/src/gui_seq_setup.rs`; in `tools/proofs/gui_list.txt`):

| sequence | frames | command log | pixels | state and events |
|---|---|---|---|---|
| `setup_pitbare@bmw_z4_gt3` | 302 | IDENTICAL | IDENTICAL | IDENTICAL (123151 lines, 1142 events) |
| `setup_pit@bmw_z4_gt3` (still) | | IDENTICAL | IDENTICAL | IDENTICAL (265161 lines) |
| `setup_pitbare` with the old `pitstop.rs` | | DIFFERENT (267 frames, from 24) | DIFFERENT (278) | DIFFERENT (121965 lines, from frame 1) |

### The pit strategy page's edits in a recording

`--record` now writes a third file beside the input file: `<input>.pit`, the pit menus' jobs
with the step each is done before (the page's presets, and the old mouse menu's clicks of
step 3). `--replay` reads it and does them at the same steps. The page only ever changes the
presets' values, so only those travel (`PitJob`, `pit_jobs_text` / `parse_pit_jobs` in
`crates/rustyac-game/src/input_file.rs`; a unit test `pit_jobs_round_trip`). A drive without
pit jobs writes no file, and a recording made before reads as one without.

### F7 and the ESC menu

The game's ESC menu does not swallow F7. `RenderWindow`'s key handler @ 0x1401fda30 calls every
listener; `KeyboardManager::onKeyDownEvent` @ 0x140243b20 calls the focused one and then every
"god listener" (`Sim` and `ksgui::GUI` both are), so nothing takes a key away. In
`Sim::onKeyDown` @ 0x14019a940 the ESC menu's test only guards the command keys (ABS, TC...);
the plain keys' `switch` comes after, and its `case 0x76` (F7) asks only
`ALLOW_FREE_CAMERA`. rustyAC does the same: the GUI gets the key and the camera code still
runs. One small difference stays: with Ctrl held and the ESC menu up the game skips its Ctrl
block and runs the plain keys, so Ctrl+F7 gives the free camera there; rustyAC needs F7 without
Ctrl (Task 43b).

### ESC in the last 5 s before an online start

The game's rule (`ESCMenu::render` @ 0x1400fe420) is for the last 5 s *before* the start, not
the first 5 s after it: with the ESC menu up then and the car not in its box
(`CarAvatar::isInPit`, 1.5 m), the car goes to the pit box and the controls are locked for 20 s.
Task 34 wrote that rustyAC did the same; online it did not: nothing gave the ESC menu the online
countdown, and the online "back to the pits" only put the car in its box. Now `OnlineGui`
carries the session type, the time to the start and "in the box" (`crates/rustyac-game/src/online.rs`,
every step in the last 6 s), and online `BACK_TO_PITS` also locks the controls for 20 s
(`crates/rustyac-game/src/sim.rs`; it says "online: ESC in the last 5 s before the start: back to
the pit box, the controls locked for 20 s").

Not shown live (Task 43b): `bash tools/web_checks/task43/s5/esc_online.sh` (Kunos' acServer on
127.0.0.1, a race of 1 lap, `--gui-do pits` to have the ESC menu up from the first frame) ran
twice and the line never came; why (the ESC menu hidden again before the countdown, or the
countdown not reaching it) is not found. The script found a bug of its own, older than this
task: a windowed rustyac.exe moves into the Assetto Corsa folder, so a relative `--out-dir`
writes there (three stray files of Tasks 42b and 43 were deleted from the install; the script
now passes an absolute path). Task 43b.

## Step 6: every skin's suits in packs

A pack now holds every skin's `skin.ini` and the driver's and crew's suit, gloves, helmet and
brand folders it names (`content::suit_files` in `crates/rustyac-web/src/content.rs`, called by
`tools/web_pack` for each skin folder of each car). The skins' own textures stay out, as before:
a car in another skin is drawn with the model's own livery, but its driver and crew now wear that
skin's suits. The page itself is unchanged: it fetches a suit only when the renderer reads it.

Preview pack (`web/preview.toml`, `--dry-run` before / after):

| | before | after |
|---|---|---|
| shared files | 1234, 140.7 MB | 1301, 182.4 MB |
| bmw_z4_gt3 | 49 files listed, 121.2 MB | 57 files, 86.0 MB |
| ks_ferrari_f2004 | 54 files listed, 179.8 MB | 51 files, 136.1 MB |
| total | 1404 files, 1743.1 MB | 1482 files, 1784.8 MB |

The per-car lines were too big before: a file both the drive and the picture read was counted
twice there (the total and the manifest never had duplicates). web_pack now counts each file once.
`--delta` of the new pack against the old one: 78 files, 41.7 MB: the Z4's 11 other `skin.ini`
and 67 suit files (crew helmets in six colours, `crew_suit/type1`, two glove and six helmet
folders, `driver_suit/sparco`). The F2004's skins name no other suits. For all 207 cars of this
install the suit folders grow from 77 (84.9 MB, first skins) to 139 (189.4 MB, every skin)
(`re/scratch/task43/s6/suits.py`, which reads the skins' `skin.ini` outside `data.acd`).

Page check (`sh tools/web_checks/task43/s6/suits.sh`): the Z4 in its `fanatec` skin at Laguna
Seca, Full mode, `window.rustyac.driveRace`, from the old pack and then the same pack with the
delta copied over it:

| pack | suit files fetched | fanatec's own (`skin.ini`, red suits, `arrow_rg7` gloves, white helmet, red crew) |
|---|---|---|
| old | 18 (the first skin's) | 0: the driver and crew in the first skin's black |
| new | 26 | 8: `skin.ini`, then its 7 suit files; the screenshot shows the red driver and crew |

Not changed:
- The player's own `Documents\Assetto Corsa\cfg\driverskin.ini`: only acs.exe reads it
  (`DriverModel::DriverModel` 0x1400f76d0); neither AC's launcher (`AssettoCorsa.exe`, its theme)
  nor rustyAC's page writes one, and the page's Documents hold only `cfg/video.ini` for the
  renderer. A pack built from one PC's `driverskin.ini` would give every visitor that PC's
  suit, so it stays out.
- The Python car values in the page: the page runs no Python apps (they need the player's
  `python33.dll`, `docs/port/python_apps.md`), so nothing would read them. The doc comment in
  `crates/rustyac-web/src/full.rs` that said rustyac.exe had none either is fixed.
- The page still fetches the first skin's suits at the start of every Full drive (about 15 MB
  for the Z4), whichever skin is drawn.

## Step 7: "Session is closed to new connections"

It is acServer's rule, not rustyAC's. The same race-only server (Laguna Seca, `WAIT_TIME=20`) was
run twice, only `[RACE] IS_OPEN` differing; desktop "a" joined 4 s after the server started,
desktop "c" and the page "b" 45 s after (`sh tools/web_checks/task43/s7/closed.sh 2`, then `1`):

| `[RACE] IS_OPEN` | desktop a | desktop c | page b |
|---|---|---|---|
| 2 | `ERROR :: Session is closed to new connections` | the same | the same (plain words shown) |
| 1 | let in, slot 0 | let in, slot 2 | let in, drives |

The answer is the server's handshake byte 0x6e (`id::SESSION_CLOSED` in
`crates/rustyac-net/src/handshake.rs`), which the game turns into that text. With a race-only
server and `IS_OPEN=2` joins were refused already 4 s after the server started (it starts in the
race's countdown); exactly when acServer closes is not checked further.

The page now says what each refusal means and keeps the game's text in a small grey line below
(`PLAIN_WORDS` in `web/app.js`; one table for the join refusals of `handshake.rs` and the kick
reasons of `online.rs`): closed session, wrong password, full, still booking, banned / blacklisted,
voted out, checksum failed (files not the server's), other protocol, auth failed, no answer.
`window.rustyac.error` stays the game's text. The panel is unchanged. Page b's text at IS_OPEN=2:
"This race has already started and the server doesn't let new players join during it. Try again
when the next session starts." / "ERROR :: Session is closed to new connections".
