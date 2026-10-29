# AC's content-maker tools (Task 44)

## Resume here

State after the last commit (update after every commit):

- **Step 1, the AI line recorder, done and proven:** `AISplineRecorder`'s recording half
  (`startRecording`, `step`, `onLapCompleted`, `beginPitLaneSpline` / `endPitLaneSpline`, `save`
  with the rebuild it does before writing, `AISpline::save` + `InterpolatingSpline::saveGrid`) is
  ported in `crates/rustyac-physics/src/track/ai_line.rs` and `spline.rs`. The car oracle runs the
  game's own recorder beside the port's (`car_oracle multi --ai-record`): the recording's point
  count every step and both saved files byte for byte. `rustyac.exe --ai-record <folder>` records
  the player's car.
- **Steps 2-7:** not started.

## In plain words

AC's "AI" app (FormAI, a developer window) records the line the AI drivers follow: you drive laps
with recording on, every 1.5 m the recorder keeps the car's position, speed, gas and brake; each
finished lap replaces the kept lap; a "pit lane" button records the pit lane the same way. When the
track is unloaded the game rebuilds the kept lap (corner radius, track edges, normals, the search
grid) and writes `fast_lane.ai.candidate` and `pit_lane.ai.candidate` into the track's `ai` folder.
rustyAC does the same and writes the same bytes, into a folder of its own.

## Step 1: the AI line recorder

### How to use it

```
rustyac.exe --track magione --car bmw_m3_e30 --ai-record <folder>
```

Recording starts with the session (in the game you press "start recording" in the AI app). Every
lap you finish replaces the kept one. When rustyac.exe ends, `<folder>/fast_lane.ai.candidate` is
written (and `pit_lane.ai.candidate` when a pit lane was recorded). To use it, rename it to
`fast_lane.ai` in the track's `ai` folder yourself, as with the game's file.

**Where the files go:** the game writes into `content/tracks/<track>[/<layout>]/ai/` (and, when a
track's line is older than version 7, overwrites `fast_lane.ai` there at load). rustyAC never
writes into Assetto Corsa's folders: the candidates go into the `--ai-record` folder, and an old
line is rebuilt in memory only.

### The proof

`car_oracle multi --scenario ai_solo --ai-record`: the game's own `AISplineRecorder` (the one
`Track::initAISpline` makes for the oracle's track) is started with `startRecording`; the port's
is fed the same car (the game's car 0, AI driven) from the engine's pre-step event, as
`Track::step` feeds the game's. After two laps the oracle calls `beginPitLaneSpline`, 6667 steps
later `endPitLaneSpline`; at the end the game's `save(false)` writes into the oracle's scratch
folder (its `dataFolder` is pointed there) and the port saves too.

| Case | Steps | Laps | Points every step | `fast_lane.ai.candidate` | `pit_lane.ai.candidate` |
|---|---|---|---|---|---|
| Spa, Formula Abarth (`ai_solo spa`) | 100000 | 2 | IDENTICAL | 2752176 bytes, IDENTICAL | 55408 bytes, IDENTICAL |
| Magione, BMW M3 E30 (`ai_solo magione bmw_m3_e30`) | 75000 | 2 | IDENTICAL | 815080 bytes, IDENTICAL | 30568 bytes, IDENTICAL |

Lists: `tools/proofs/replay_list.txt` (both), `tools/proofs/quick/replays.txt` (Spa).

Live check: `rustyac.exe --headless --track magione --car bmw_m3_e30 --autodrive --no-race-ini
--duration 260 --ai-record re/scratch/task44/live/ai` wrote both candidates into that folder and
nothing anywhere else. The test autodriver finished no lap in that time (it cut the track), so these
are the loaded lines rebuilt and saved again, which is what the game saves after a recording with no
finished lap too (the oracle's 3000-step smoke run proves those bytes).

Found on the way: the port read a track's pit lane without its search grid (the fast reader), the
game reads it with the grid; a re-saved pit lane differed. Fixed: the pit lane is read as the
game reads it.

### Not done

- In the game the kept lap becomes the track's live AI line at once (`bestLapSpline` is the line
  `Track` hands the AI and the cars' position on the track); in rustyAC the loaded line stays live
  until the next session. With `--ai-record` the whole-car compare therefore differs from the first
  finished lap on (`car0.npos`); the proof judges the files only. Task 44b.
- The AI app's window (FormAI: the buttons, the hints editor, the drawn splines) is not ported;
  rustyAC records from the start of the session.
