# Task 42c: leftovers sweep

## Resume here

- Done: steps 1 to 4 (the test autodriver, an online race to its end, Python apps, the page's
  keys and hidden launcher).
- Next: step 5 (the mouse pit stop's seed 4 that differs between recordings).
- Scratch: `re/scratch/task42c/`.

## Step 1: the test autodriver (`--autodrive`)

What was wrong:

- After "back to pits" it never used the pit lane again. It picked the pit lane's line only
  until it first reached the racing line, and that choice was never made again.
- A GT3 car got stuck. At Laguna Seca the BMW Z4 planned to brake as hard as a Formula 1 car
  (16.8 m/s²) but slows at about 13. It arrived too fast, spun in the corkscrew and stood in the
  gravel at full lock with the gas down for good (0 km/h, gear 6, 59 % of the lap). The F2004
  laps the same track fine.

What changed (`crates/rustyac-game/src/autodrive.rs`):

- A jump of more than 10 m between two steps (back to pits, back on track, a new session)
  starts it again from where the car stands, so the pit lane's line is chosen again.
- It measures every 0.25 s of full brake above 10 m/s. A car that slows less than planned
  brakes earlier from then on, and turns the wheel less at speed. A car that slows as planned
  (the F2004) drives exactly as before.
- Stuck (it has driven, then stands below 1 m/s for 3 s with the gas down): it sends the
  game's "to track" (`event::TO_TRACK`), as a player would. The page's driver passes that on
  too (`crates/rustyac-web/src/driver.rs`).

Proof:

| Check | Result |
|---|---|
| F2004, Spa, 300 s `--autodrive --record` with v0.42.1 and the new exe (100,464 steps) | byte-identical `.ryin` |
| New test `after_back_to_pits_the_line_follower_takes_the_pit_lane_again` (GT86, Magione, pit box, twice) | passes |
| New test `a_gt3_car_laps_laguna_seca` (Z4, 360 s, 2 laps or more) | passes: 2 laps (it got stuck twice, at 173 s and 370 s of a 400 s trial run, and went back on track both times) |
| The other `track` tests | pass |

The recordings the proofs replay (`oracle/game/spa_autodrive_300s.ryin`, the replays area's
60 s F2004 drive) are F2004 drives, so they do not change.

Not done: the Z4 still runs wide in a few bends at Laguna and gets stuck about once a lap. It
now carries on. A driver that never gets stuck would need per-car grip, which a test driver
does not need.

## Step 2: an online race to its end (page and desktop)

`sh re/scratch/task42c/s2/live.sh 1` (from Task 42b's): a race-only copy of Kunos' acServer
(1 lap, Laguna Seca, 20 s result screen) on 127.0.0.1 with the gateway and `web/serve.py`.
Desktop "a" (F2004, `--autodrive`) races from the start; page "b" (Full mode, `autodrive=1`)
joins 8 s later and races too. The page's flow (`mkflow.py`) waits for the log lines below,
clicks the ESC menu's Drive (50, 172) in the pits and again after the next session begins.

What was wrong:

- The page never passed the client's news to AC's GUI: the race's end, the next session, chat
  lines, votes and server messages reached the desktop's GUI only. The page's car went to its
  pit box but no ESC menu came up, and nothing held the car. Now `GuiLoop::exchange`
  (`crates/rustyac-web/src/full.rs`) hands them over, and sends the GUI's chat lines and votes
  to the client, as rustyac.exe's loops do.
- Desktop and page: at the next session the pit stop's handler (`PitStop`, lambda
  @ 0x1400a7ce0) frees the controls. AC's ESC menu, still up, locks them again on its next
  frame; rustyAC did not, so the car drove off at the next start without Drive (seen: 254 km/h
  at the end of the run). The online "new session" now marks the lock stale
  (`crates/rustyac-game/src/render/gui.rs`), as the offline one did.
- The client now prints three lines for checks: `online: the results came (clock …)`,
  `online: the race is over: the car goes to its pit box (clock …)`,
  `online: a new session: <name> (clock …)`.

| Run | Results | Pit box | Next session | Drive in the pits | After the next start |
|---|---|---|---|---|---|
| desktop a, no Drive (`ASECS=295 DRIVE_AT=900`) | 234.5 s | 239.5 s (+5.0) | 254.6 s | not pressed | 0.0 km/h, brake 1, 30 s after the start (held) |
| desktop a, `--pit-key drive@262` (`run_drive/`) | 224.0 s | 229.0 s (+5.0) | 244.2 s | at 262 s | 64.7 km/h at the end (racing) |
| page b (the two runs) | 110.0 / 112.7 s | 115.0 / 117.7 s (+5.0) | 131.2 / 133.7 s | clicked: 0 km/h 4 s later, ESC menu still up (`b_drive_refused.png`) | clicked after the new session: on the grid until the start, then 214 km/h |
| before the fix, desktop a, no Drive | 223.7 s | 228.7 s | 243.9 s | — | 254.1 km/h (drove off without Drive) |

The page's times are its own clock (it joined later). Shots: `run/b_pits.png` (the ESC menu in the pit box), `b_new_session.png`, `b_drive_again.png`.

Not done: options.ini's `IGNORE_RESULT_TELEPORT` and the fade (Task 42b's shortcut) stay.

## Step 3: Python apps

What changed:

- `setCameraMode` (cockpit, car, drivable, free), `setCameraCar`, `focusCar`, the free camera's
  moves and turns and `sendChatMessage` are carried out in rustyac.exe (`render/gui.rs` hands
  them to `desktop.rs`). The free camera functions move the same camera F7 gives
  (`MouseCamera` in `render/scene.rs`, now with the game's `moveForward`, `moveRight`,
  `moveUpWorld`, `rotatePitch/Heading/Roll`). Online, a chat line goes to the server, with
  the game's one-second limit (a second line within a second returns -1).
- Online, `addOnChatMessageListener` keeps the callback, and every chat line the client hears
  calls it with (message, name). The name is "SERVER" for the server's lines, else the
  sender's driver (lambda 0x140126860). Offline it keeps nothing, as in the game.
- `getServerName/IP/HttpPort` read the `[REMOTE]` section the game reads (`SERVER_NAME`, the
  address, `SERVER_HTTP_PORT`), and `getServerSlotsCount` the connected cars.
- `addOnAppActivatedListener` / `addOnAppDismissedListener` put a handler on the app bar's
  "opened" / "dismissed" events. It calls back with the app's index when its icon is pressed
  (0x14012f120, 0x14012f240, lambda 0x140126c40).
- The car values come from the session: `getCarBallast`, `getCarRestrictor`
  (`roundf(restrictor * 400)`), `isAIControlled`, `getCarMinHeight` (the player's car only:
  `car.ini [RULES] MIN_HEIGHT`, -1 without), `getCameraCarCount` (the car's `cameras.ini`),
  and the track's sector count (the splits).
- `newApp(name, 1)`: no change. In acs.exe 1.16 it makes the form and fires
  `evOnSetupAppCreated`, but nothing in the game listens to that event, so the form is on no
  screen. rustyAC already did the same. Showing it on the setup screen is what Content
  Manager / CSP add, not the game.

Proof:

| Check | Result |
|---|---|
| Python proof area, `py_quick` and `py_apps` (AC's PythonInterface vs rustyac-python) | 2 of 2 identical |
| `py_apps` now also opens the app bar and presses the test app's icon twice | the game and the port both call "dismissed (1)" then "activated (1)"; 120 frames, draws, pixels, state and the apps' logs identical |
| rustyac.exe offline, Magione, 2 cars, check app `cam42c` (`re/scratch/task42c/s3`) | car camera 1 at 1 s, free camera moved back and up at 2 s, focus on car 1 at 3 s (pictures in `re/scratch/task42c/s3/out`); AI car reads `isAIControlled` 1, the player's 0; minimum height -1.0 for the player, -1 for the others; 6 cameras each |
| rustyac.exe online vs a local acServer copy (`s3/online.sh`) | `sendChatMessage` goes out; the listener fires for the app's own line and for desktop b's "hello from b" (name "b"); `getServerIP` 127.0.0.1, slots 2; splits padded to Laguna's 3 sectors |

Not done: track, helicopter, on-board free and random cameras for `setCameraMode` (3, 4, 5,
7) and `freeCameraSetClearColor` are still only logged. Neither camera exists in rustyac.exe.
The name the listener gets for the player's own line is the one `getDriverName(0)` gives
("Player" unless race.ini names the driver).

## Step 4: the page's F4 / F7, the hidden launcher, full-screen Esc

What changed:

- **F7 in Full mode** is AC's free camera when the game's `system/cfg/assetto_corsa.ini`
  allows it (`[CAMERA] ALLOW_FREE_CAMERA=1`, read as rustyac.exe reads it). It starts where the
  picture's camera was; the arrows move it (Shift slower, Ctrl faster), the right mouse button
  held turns it, F1 or F6 go back to the driving cameras. Without the permission (the usual
  install) F7 is still the page's "skip the session".
- **F4 in Full mode**: back to your car, through AC's fade (`fades.ini [CHANGE_CAMERA_FADE]`,
  only with other cars). The page's camera never leaves your car, so only the fade shows.
  `Focus::new` now reads `fades.ini` through the page's file system too.
- **The hidden launcher stops.** AC's theme asks `ac://eventcount` every second (its Special
  Events page) and every answer saved the player's documents. While a drive hides the
  launcher, `theme_glue.js` clears the theme's intervals and holds its timeouts; they start
  again when the launcher shows.
- **Full screen**: the key lock was already there (Task 42b). The controls page now says that
  Esc opens AC's ESC menu in full screen too, and that holding Esc leaves full screen.

Proof (`re/scratch/task42c/s4`):

| Check | Result |
|---|---|
| Timers and rAF per 10 s by calling file, Full drive (`cost.sh before` / `after`) | launcher shown: 20 `launcher.js` timeouts (the theme's 1 s poll) both times; driving: 20 before, **0** after (the last bridge request is `ac://start`) |
| The same with a Debug drive left with Esc (`cost.sh after_esc`) | back in the launcher the poll runs again: 20 per 10 s |
| The page on a local acServer copy, Full mode, a stand-in install that allows the free camera (`live.sh`) | F7: the picture stays where it was; ArrowUp 0.3 s: the camera moved past the car; F1: the chase camera again; F4: one dark frame (mean 11 of 255), then the picture again |
| Full screen there (`requestFullscreen` with a user gesture) | in full screen, the lock is there; Esc reached AC's GUI ("Stop for Esc Menu": the car was moving) |

Not done: holding Esc to leave full screen is the browser's own; headless Chrome stays in full
screen, so it was not seen. Free camera without the permission does nothing in AC; the page
keeps F7 as its skip key then (a page-only key since Task 28).
