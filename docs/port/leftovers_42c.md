# Task 42c: leftovers sweep

## Resume here

- Done: step 1 (the test autodriver), step 2 (an online race to its end).
- Next: step 3 (Python apps: camera, chat, listeners, setup-screen apps, car values).
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
