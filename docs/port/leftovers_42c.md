# Task 42c: leftovers sweep

## Resume here

- Done: step 1 (the test autodriver).
- Next: step 2 (a short online race to its end, page and desktop, local acServer).
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
