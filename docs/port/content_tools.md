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
- **Step 2, DRS zones, done and proven:** the zones' reading and logic (`DRSManager`) were ported
  before (car oracle `spa_drs_timing`); the HUD's DRS lamp, the Electronics app's DRS light and the
  cockpit's DRS instruments too. New: the developers' "DRS Zones" window (`FormDRSZones`), proven
  in the GUI oracle (`app_drszones`: pictures, state, the saved file and the messages).
- **Step 3, camera editors, in progress:** the developers' "ChaseCam Editor" (`ChaseCamEditor`)
  done and proven in the GUI oracle (`app_chasecam`). The camera editor (`FormCamera`) next.
- **Step 6, in progress:** the developers' "Net monitor" (`NetMonitorForm`) done and proven
  (`app_netmonitor`). The input debug apps next.
- **Steps 4, 5, 7:** not started.

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

## Step 2: DRS zones

### What was already there

- `data/drs_zones.ini` (`DRSManager::DRSManager` 0x140278ea0) and `DRSManager::isDRSAvailable`
  0x140279490: `crates/rustyac-physics/src/track/` (`DrsZone`, `Track::is_drs_available`), proven
  by the car oracle's `spa_drs_timing`.
- The HUD's DRS lamp, the Electronics app's DRS light, the cockpit's `DRS_*` instruments.
- `DRSManager::step` and `wasRaceSwitchedOn`: nothing to port. The game's `Track::step` calls a
  linker-folded empty function for the step, so the manager's `detections` stay empty and
  `wasRaceSwitchedOn` is always false.

### The "DRS Zones" window (`FormDRSZones`)

`crates/rustyac-gui/src/app_drs_zones.rs`. A developers' app (shown with `ENABLE_DEV_APPS`): the
player's car's position along the lap, one button that marks a zone's detection point, start and
end in turn, the zones' list, and "Save".

How to use it: drive, press the button at the detection point, at the start and at the end of each
zone, then Save. The game's constructor leaves the button at "end" under its "Set Detection" text,
so the first press ends a zone that begins at 0 (as in the game). Save writes
`content/tracks/<track>[/<layout>]/data/drs_zones.ini.candidate` in the game; rustyac.exe writes it
under its own cfg folder: `<cfg>/maps/<track>[/<layout>]/data/drs_zones.ini.candidate` (the folder
where the track map's files go too). Rename it to `drs_zones.ini` in the track's `data` folder
yourself.

Proof: `render_oracle gui --gui app_drszones` (in `tools/proofs/gui_list.txt` and the quick list):
71 frames, command log, pixels, state and events IDENTICAL; the saved file (169 bytes, three
zones) and the window's two messages ("Nothing to save", "DRS Zones saved") IDENTICAL.

Found on the way: a GUI oracle run of an app sequence alone failed while staging (no `cfg` folder
in a fresh root); the staging now makes it.

### Not done

- The zones a server sends (0x53, `ACClient::onDRSZoneReceived` 0x14004b520 then
  `DRSManager::setZones`): decoded, not acted on; rustyAC keeps the track's own zones online.
  Task 44b.

## Step 3: the camera editors

### The "ChaseCam Editor" (`ChaseCamEditor`)

`crates/rustyac-gui/src/app_chase_cam.rs`. A developers' app (shown with `ENABLE_DEV_APPS`).
Three spinners move the first chase camera while it is on screen: distance and height in tenths of
a metre, pitch in whole degrees. Another camera on screen: the spinners and buttons are hidden and
the window says "Select Chase Cam 0 to activate". "Save CHASE_0" and "Save CHASE_1" both write the
first chase camera's values (as the game does) into `chase_cam.ini` under that section, with
`WritePrivateProfileStringW` (values as `%f`), then show "Global chase_cam.ini saved for CHASE_n".

Where the file goes: the game writes `system/cfg/chase_cam.ini` in its own folder. rustyac.exe
writes `<cfg>/system/cfg/chase_cam.ini` (made from the game's file the first time), and reads the
chase cameras from that file when it exists, else from the game's (the game's
`CameraDrivableManager::initChaseCamFromINI` 0x1400c61c0; rustyAC had the shipped values built in
before).

Proof: `render_oracle gui --gui app_chasecam` (in `tools/proofs/gui_list.txt` and the quick list):
47 frames, command log, pixels, state and events IDENTICAL; the saved `chase_cam.ini` (130 bytes:
distance 3 clicks up, height 2 down, pitch 4 up and 1 down, saved as CHASE_0 and CHASE_1) and the
messages IDENTICAL; a Save click with another camera on screen does nothing on both sides.

## Step 6: the net monitor and the input debug apps

### The "Net monitor" (`NetMonitorForm`)

`crates/rustyac-gui/src/app_net_monitor.rs`. A developers' app, made online only (in `Sim::Sim`
after the pedals' app, when there is an `ACClient`). A graph (400 x 200, up to 400 values, top
300) of the speeds in km/h of car 0 (white) and car 1 (magenta); with a second car, "EV: " and the
length of car 1's error vector (the gap between where its last packet put it and where it is
drawn; a stream of precision 2: `0.005`, `1.2e+003`) and "P: " and the client's own ping
(`ACClient::getPing(car 0)` is the client's, not the car's). The labels sit on the graph, as in
the game. It writes nothing.

In rustyac.exe the ping and car 1's (guid 1) error vector come from the online state the physics
thread hands over every 64 steps (shortcut: not every frame; the game reads the live values).

Proof: `render_oracle gui --gui app_netmonitor`: 68 frames with five pings and error vectors from
0 to 1234.5 and the window hidden and shown, command log, pixels, state and events IDENTICAL. The
game side runs the real constructor and render with a stand-in client (its avatar car 0, its
socket's ping) and a block with `NetCarStateProvider`'s vftable as car 1's state provider, for the
render's dynamic cast.
