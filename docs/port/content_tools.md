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
- **Step 3, camera editors, done and proven:** the developers' "ChaseCam Editor"
  (`ChaseCamEditor`, `app_chasecam`) and the "Camera editor" (`FormCamera`, `app_camera`: pictures,
  state, every frame's camera settings and the three saved files).
- **Step 6, done and proven:** the developers' "Net monitor" (`NetMonitorForm`,
  `app_netmonitor`), "Keyboard debug" (`KeyboardControlsDebug`, `app_keyboard`,
  `app_keyboard_none`) and "X360 Controller debug" (`X360ControllerDebug`, `app_x360`,
  `app_x360_none`).
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

### The "Camera editor" (`FormCamera`)

`crates/rustyac-gui/src/app_camera_editor.rs`. The game makes it only with YEBIS's camera, so only
with post-processing on (video.ini `[POST_PROCESS] ENABLED=1`, or `rustyac.exe --post 1`); it is a
developers' app. What it does:

- **F1-1 ... F1-6** pick the cockpit and the driving views (F1-2 and F1-3, the chase cameras, are
  hidden); **F6-1 ... F6-6** the car's own cameras. When it opens, the game copies the car's first
  camera until the car has six, so all six F6 buttons work.
- **Pitch / Roll / Yaw**, **Elev / Strafe / Move**: the cockpit's eye point and pitch, the bonnet's
  and bumper's places and pitches, or a car camera. Steps of 0.1 mm, 1 mm or 1 cm
  (**SENSITIVITY** -/+, one to three red bars); turns of 0.01, 0.1 or 1 degree for car cameras.
- **EXP -/+**, **FOV -/+**, **External / Internal**: the exposure of the view, the field of view and
  the car camera's sound.
- **SET** keeps the camera where it is now as the car camera. **SAVE CAMERAS** writes car.ini
  `[GRAPHICS]` (both exposures, and for the view on screen the eye point and pitch, the bonnet's or
  the bumper's place and pitch) or, on F1-6, dash_cam.ini `[DASH_CAM] EXP`, then cameras.ini (`[CAMERA_n]` POSITION / FORWARD / UP / FOV / EXPOSURE / EXTERNAL_SOUND).
- **AUTO EXP** goes to the cockpit and, every 3 seconds, nudges each view's exposure by YEBIS's
  measured brightness until it settles, then saves.

Where the files go: the game writes into `content/cars/<car>/data/` in its own folder. rustyac.exe
writes the same three files into `<cfg>/content/cars/<car>/data/` (car.ini made from the game's
file the first time, as the Onboard settings app does).

In rustyac.exe the editor's changes move the real views at once: the car cameras, the bonnet and
bumper views, and the eye point and pitch (through the Onboard settings' actions). The exposures
are kept and saved but change nothing on screen (rustyAC's renderer has no YEBIS exposure yet).

Proof: `render_oracle gui --gui app_camera` (in `tools/proofs/gui_list.txt` and the quick list):
1153 frames: command log, pixels, state and events IDENTICAL; car.ini (1544 bytes), dash_cam.ini
(20 bytes) and cameras.ini (703 bytes) IDENTICAL; every frame's camera settings (exposures, the six
cameras' matrices, field of view and sound, the eye point, the views, the scene camera, bit for
bit) and the camera manager's calls IDENTICAL (90372 bytes). The sequence moves and saves the
cockpit, bonnet, bumper and dash views, edits and SETs a car camera, changes the sensitivity, and
runs AUTO EXP over 1020 frames with YEBIS's measured exposure changed four times. The oracle stands
in for the camera manager (its calls are written down, not made) and YEBIS (the measured exposure
comes from the script). Checked in rustyac.exe too: `rustyac.exe --track magione --car bmw_m3_e30
--no-race-ini --post 1 --screenshot cam.png --gui-do app:cameraeditor` shows the window.

Not done (Task 44b):
- rustyac.exe does not read its own saved cameras.ini / dash_cam.ini back on the next start (the
  game reads its edited files; rustyAC reads the game's).
- `[AC_APPS] AUTOEXP` in the game's assetto_corsa.ini starts AUTO EXP and the game quits
  (`exit(2)`) once it has saved; rustyac.exe reads the value but does not quit.
- What `ACCameraManager::setMode` does besides switching the view (the game's free camera, track
  cameras) was not traced.

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

### The "Keyboard debug" app (`KeyboardControlsDebug`)

`crates/rustyac-gui/src/app_keyboard_debug.rs`. A developers' app, always made (in `Sim::Sim`
after the race position app). 600 x 600, the keyboard's steering parameters in red Consolas:
"Steering Speed", "Opposite Direction Speed", "Steering Gain", "Steering Reset", "LookAhead
Points", the three decelerating factors, the two integrators ("Int Gas", "Int Steer") and "Min
Steer", each floored to four decimals. The first five have spinners (0 to 100) that change them at
once while driving: speed = value x 0.05, opposite direction and reset = value x 0.1, gain = value
x 0.01, look-ahead as is. When the car's controls are not the keyboard's class the window says
`KEYBOARD CONTROL NOT SELECTED : 0` and has no spinners. It writes nothing (the changes are lost at
the end of the session, as in the game).

In rustyac.exe the window shows the live keyboard class of the physics thread (published every
step); the spinners' changes go back to it through the shared state. rustyAC switches devices
while driving where the game picks one class at the start: the window counts as "selected" when
rustyAC starts on the keyboard (no pad, or `INPUT_METHOD=KEYBOARD`), as the game would. With
`--autodrive`, `--replay` or `--screenshot` there is no keyboard class: "NOT SELECTED".

Proof: `render_oracle gui --gui app_keyboard` (41 frames: the shipped `[KEYBOARD]` values, every
spinner clicked down and up, the integrators moving, the window hidden and shown) and
`app_keyboard_none` (9 frames, no keyboard class): command log, pixels, state and events
IDENTICAL. The game side runs the real constructor, render and spinner handlers; the oracle's
`getControlsProvider` stand-in answers a block with `KeyboardCarControl`'s vftable (for the
constructor's dynamic cast) while the window is made. A `rustyac.exe --screenshot ... --gui-do
app:keyboard` picture shows the window drawn ("NOT SELECTED": no keyboard class in a screenshot
run).

### The "X360 Controller debug" app (`X360ControllerDebug`)

`crates/rustyac-gui/src/app_pad_debug.rs`. A developers' app, always made (in `Sim::Sim` after
the track description). 400 x 300. With the Xbox pad's class as the car's controls it prints, in
the GUI's own font (its colour changed for good, as in the game), "RAW", "Filtered" and "% of
steer" in yellow (the stick's last raw value, its filtered value, the last steering limit), "Steering
Speed", "DeadZone", "Steer Filter" and "Steer Gamma" in magenta and "Speed Sens." in white. Five
rows of - and + buttons (`LRControl`: "Speed Sens", "Steer Gamma", "Steer Filter", "Deadzone",
"Speed") change the pad's parameters by 0.001 (Steer Gamma by 0.01) at once. "Save" writes
`STEER_GAMMA`, `STEER_FILTER`, `SPEED_SENSITIVITY`, `STEER_DEADZONE` and `STEER_SPEED` (`%g`) into
`[X360]` of Documents' `cfg/controls.ini` and shows "X360 Parameters" / "Saved". The values it
saves are the class's own: `STEER_FILTER` and `STEER_SPEED` are written as the class keeps them
(after its reading's arithmetic), so a save and a reload do not give back the same steering, as in
the game.

Where it writes: the game writes `Documents\Assetto Corsa\cfg\controls.ini`. rustyac.exe writes
its own `cfg/controls.ini` (in rustyAC's cfg folder, through the write guard), starting from a copy
of the Documents file the first time.

Without a pad class the window is empty below its buttons; the game's buttons then write through
a null pointer (a crash), the port's do nothing. In rustyac.exe the class counts as the game's when
rustyAC starts on the pad (a pad found and `INPUT_METHOD` not `KEYBOARD`).

Proof: `render_oracle gui --gui app_x360` (43 frames: every row clicked down and up, the stick's
values moving, Save, the window hidden and shown; command log, pixels, state and events IDENTICAL;
`cfg/controls.ini` 163 bytes IDENTICAL; the messages 30 bytes IDENTICAL) and `app_x360_none` (9
frames, no pad class, IDENTICAL). The game side runs the real constructor, render, buttons and Save
with a block carrying `JoypadCarControl`'s vftable answered by the `getControlsProvider` stand-in.
