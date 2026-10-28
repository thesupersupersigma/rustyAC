# Task 43b: never write into the AC folder, and Task 43's leftovers

## Resume here

- Done: steps 1 to 4, released as v0.43.1 (step 3's online mouse menu not done, see Not done).
- Scratch: `re/scratch/task43b/`. Rerun scripts: `tools/web_checks/task43b/`.

## Step 1: one guard for every write

A window run of `rustyac.exe` changes its working folder to the AC install (the Python apps need
it), so any relative path option was read against the AC folder. Two fixes:

- Every path option is made absolute while the command line is read, before anything changes
  folder (`absolute` in `crates/rustyac-game/src/cli.rs`; `--dump-states -` stays stdout).
- Every file write of rustyAC's crates goes through `rustyac_content::guard`
  (`crates/rustyac-content/src/guard.rs`): `write`, `create`, `append`, `create_dir_all`,
  `create_dir`, `remove_file`, `remove_dir_all`, `rename`, `copy`. Each refuses, with
  "refusing to write X: it is inside ...", a path inside the AC install or inside
  `Documents\Assetto Corsa` (the real one: rustyAC writes there nowhere). The check makes the
  path absolute, folds `..`, follows the deepest existing folder to its real path (junctions)
  and compares without letter case. The Python apps' `py_log.txt` and the `__pycache__`
  clean-up go through it too.

Test: `crates/rustyac-game/tests/write_guard.rs`. A made-up AC folder; relative and absolute
`--out-dir`, `--cfg-dir`, `--documents`, `--replay-dir`, `--setups-dir`, `--screenshot`,
`--telemetry-dir`, `--save-replay` all become absolute in the test's own folder; then, from
inside the made-up AC folder, writes of `out/race_out.json`, `cfg/race.ini`,
`logs/py_log.txt`, a replay, a setup, a screenshot, `./system/../x.txt`, an upper-case path and
the `__pycache__` delete are all refused, and nothing was written into it.

Not guarded: what a Python app's own Python code writes (it runs inside the player's
python33.dll).

### The AC folder (read only, nothing deleted)

Every file of `C:\Program Files (x86)\Steam\steamapps\common\assettocorsa` changed since the
repo's first commit (2026-10-05 14:57): 5,908 files in 39 new folders, lists in
`re/scratch/task43b/ac_after_start.txt` and `ac_dirs_after_start.txt`. All are mods, Custom
Shaders Patch or Content Manager files: car packs (`bdc_streetspec_*`, `vrc_formula_*`,
`rss_formula_hybrid_2026`), tracks (`bahrain`, `vhe_interlagos`, ...), Lua apps, CSP's
`cache\` and `extension\`, `dwrite.dll`. The `body_shadow.png` / `tyre_N_shadow.png` files in
car folders came with those car packs (rustyAC writes its shadows into its own
`rustyac_cache\shadows`, and did since that code exists). No `__pycache__`, `py_log.txt`,
`race_out.json`, `laps.ini`, `out`, `logs`, replays, setups, screenshots or `re` folder. None
of it looks written by rustyAC or its tools.

## Step 2: the live checks

### a. The online ESC lock

The cause was in rustyAC: the online session never took the server's race start, so the time
to the start the ESC menu reads was always negative and its "last 5 s" branch never ran
(`crates/rustyac-game/src/online.rs`, `Event::RaceStart`: now `self.session.start_time =
*start`, as `RaceManager::setCurrentSessionStartTime`). The HUD's "start in" reads the same.
The test script had a bug too: `MSYS_NO_PATHCONV` with a `/c/Users/...` path sent client a's
`race_out.json` to `C:\c\Users\...` (fixed with `pwd -W`; the two stray files there are
yours to delete).

Live run (`tools/web_checks/task43/s5/esc_online.sh`, Kunos acServer, Laguna, 1-lap race):

| | result |
|---|---|
| "ESC in the last 5 s before the start: back to the pit box, the controls locked for 20 s" | once, at 4.9 s before the start |
| car in its pit box | from 4.5 s before the start on |
| race_out.json a / b | Race [(1, 121287)] / [(0, 121287)] |

### b. A live pit stop through the mouse menu

The game's AI never uses the mouse menu: with a pit request it drives the pit lane and puts its
car into the box itself (`AIDriver::stepSplineSelection`). Its own stint call is skipped in a
race's last two laps; a call from outside (`--ai-pit-at`) is not, but one made in the last lap
came after the finish in Task 43's run. So: `--ai-drive` drives a keyboard driver's car (so
`--pit-click` gives the mouse menu), called in at 150 s of a 6-lap race at Magione, and
`--pit-click` picks tyre 2, fuel up twice, body repair and CONFIRM once the menu is open.

`SESSIONS=r6 SECS=300 FROM=211 TO=212 DRIVE="--ai-drive --ai-pit-at 150" sh tools/web_checks/task43b/pit_live.sh`:

| | live | replay 0 workers | replay 1 | replay 8 |
|---|---|---|---|---|
| pit lane, menu open | 194.5 s, 210.5 s | | | |
| stop | 10.7 s, tyres SM -> ST | same | same | same |
| state hash | | 6d7dca190c323f2a | 6d7dca190c323f2a | 6d7dca190c323f2a |

An earlier round of clicks every 5 s happened to open the menu between body repair and
CONFIRM: a CONFIRM with only body repair on an undamaged car ends the stop at 0 s, as the game
does.

### c. The page's online results page

`bash tools/web_checks/task43b/s2c/live.sh`: Kunos' acServer (race only, Laguna, 2 laps), the
gateway and `web/serve.py`, all on 127.0.0.1; desktop "a" (F2004, autodrive) races; the page
"b" (Full mode, autodrive) starts in AC's launcher and the script clicks in the theme's own
frame: Drive -> Online, OK on the multiplayer agreement, the address bar's server (never the
site's real ones), the Ferrari, Join. After the race the page leaves and is back on the
launcher.

| | server log | page's race_out.json (launcher's `getresults`) | launcher |
|---|---|---|---|
| a | 2:10.336, 1:18.117 | 130336, 78117, 0 cuts | 1st, 3:28.453, 2 laps |
| b | 1:47.191 | 107191, 0 cuts | 2nd, +1 L, 1 lap |
| fastest lap | | bestLaps a 78117 lap 1, b 107191 | "Fastest lap by a 1:18.117 on lap 2" |

The launcher first showed "NEW PERSONAL BEST, Ferrari F2004 @ Laguna Seca" for b's
lap, then, after its continue, the results page above. No JavaScript error.

A first run with a 1-lap race found that AC's theme cannot show a race whose laps are all cut:
`results.js` reads `session.bestLaps[0].car` whenever a race has laps, and `bestLaps` lists
valid laps only (as `JsonOutputFile` does). a's lap out of the pits had a cut, so the page got
"TypeError: Cannot read properties of undefined (reading 'car')" and no results page. The same
file would do the same in AC's own launcher, so rustyAC keeps it; the script races 2 laps.

## Step 3: small items

- **The page's keyboard drivers get the old mouse pit menu** (Full mode with AC's GUI, a drive
  from AC's launcher): `Session::from_race(.., keyboard)`; the clicks go to the sim as on the
  desktop. Without AC's GUI (Debug mode) the quick menu stays: nobody could click the other.
  Test: `a_keyboard_driver_gets_the_mouse_pit_menu` in `crates/rustyac-web/tests/drive.rs`.
  The page does not read `pitstop.ini USE_MOUSE_PITSTOP` (the desktop does).
- **The pit marker only in the pit lane** was already ported (Task 43's note was wrong):
  `[PIT_INDICATOR] ONLY_IN_PITLINE` is read in `crates/rustyac-game/src/render/ac.rs` and
  `pit_crew.rs render_hud` hides the marker outside the pit lane. The game also hides it while
  its ESC menu shows; rustyAC's renderer does not know the ESC menu (only the pause menu).
- **Ctrl+F7 with the menu open:** `Sim::onKeyDown` skips its Ctrl block while the pause menu
  shows, so Ctrl+key runs the plain key there (Ctrl+F7 = F7, the free camera). Now the same in
  `desktop.rs`. It is the pause menu, not the online ESC menu.
- **Docs:** `--help` now says `--autodrive` and `--ai-drive` keep the quick pit menu (they click
  no CONFIRM) unless `--pit-click` is given.

## Step 4: `web_pack` and letter case

`tools/web_pack` now spells every folder the way the first manifest path that names it does,
and writes every file under exactly its manifest path; a folder already on disk in other
letters (from an earlier run) is renamed to the manifest's. Before `manifest.json` is written,
two paths (or folders) that differ only in letter case stop the pack with an error. Test:
`every_folder_has_one_spelling_on_disk_too`.

## The proofs at the end

`bash tools/proofs/run_all.sh --quick` on the release's code: every area 0 different.

| area | cases | identical | different |
|---|---|---|---|
| physics | 9 | 9 | 0 |
| fresh | 47 | 47 | 0 |
| sound | 4 | 4 | 0 |
| render | 10 | 10 | 0 |
| gui | 15 | 15 | 0 |
| python | 1 | 1 | 0 |
| replays | 5 | 5 | 0 |
| net | 16 | 16 | 0 |
| browser | 9 | 9 | 0 |
| ai_page | 3 | 3 | 0 |
| shaders | 4 | 4 | 0 |
| launcher | 1 | 1 | 0 |
| dumps | 6 | 6 | 0 |

A first run after step 1 found two of the guard's own: `--dump-states -` had become a file
named `-` (fixed), and `tools/chassis_compare` did not build (it includes rustyac-game's
sources and now names rustyac-content). Plus the proofs of what changed: `write_guard` and the
crates' tests, `web_pack`'s test, the page test above, the three live runs. Clippy as CI runs
it (native `--all-targets`, wasm32-unknown-unknown, wasm32-wasip1, web_pack): clean.

## Not done

- The mouse pit menu online. rustyAC's online drive draws no pit menu in AC's GUI at all (its
  pit stop is `OnlinePit`'s quick menu, shown as HUD lines); putting the referee's pit page on
  screen online is its own task.

## Open questions

- None new.
