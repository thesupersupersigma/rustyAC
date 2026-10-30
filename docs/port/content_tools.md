# Task 44c: the leftovers of Tasks 43c, 44 and 44b

## Resume here

- Steps 1 to 9 are done, each in its own commit. Step 10 (only "if time is left") was not
  started and moves to Task 44d with the rest of "Not done" below. Released as v0.44.2.

## In plain words

The web page no longer asks `ac://eventcount` every second. That request is the Special Events
page's loading percentage, and it only kept going because the page could not see the special
events folder. A lap recorded with the AI app now becomes the track's AI line straight away, as
in the game, so whole-car runs that record agree with the game past the first lap. The AI app's
own window (FormAI) is ported. In photo mode, the END key measures the distance to what is in
the centre of the screen, using the game's own ray picker, and the exposure multiplier starts
from `exposure.ini`. Online, the server's DRS zones replace the track's and the shared memory's
online-only static fields are filled in. The shared memory's graphics page during a replay now
matches the game's. The console's `axisMode` draws the world's axes. The Suspensions window's
TempRun button writes its file. In the browser, the old graphics list is back without the
launcher, the rustyAC tab has an icon, and the changelog and the OK button of notices work. What
rustyAC writes to `race_out.json` after a server starts its next race already matched what the
game writes.

## Step 1: the page's `ac://eventcount`

AC's theme (`modules/specialevents/specialevents.js`) asks for `ac://listspecialevents?async=true`.
It then asks for `ac://eventcount` once a second to show a loading percentage, and the list's
answer stops that timer. In the real launcher the list comes back in well under a second
(`ACSchemeHandler.cs:122`: `async=true` runs on a worker thread; `ACRequestHandler.cs:833`
`ListSpecialEvents`, with `GetEventCount` giving the percentage), and the same is true in desktop
rustyAC. So `eventcount` is never asked there. The page's folder copy had no `content/specialevents`,
so the answer was an error and its JSON never parsed. The callback never ran and the timer never
stopped. The real launcher would do the same without that folder.

Fix: `web/launcher.js` now reads `content/specialevents` (one level deep), and `tools/web_pack`
packs every event's `event.ini` (203 files, about 0.8 MB). Their pictures (144 MB) are left out,
with a `shortcut:` comment. A page run after the fix shows `listspecialevents`, "Loaded 203
special events" and `getachievements`, and no `eventcount` at all, as on the desktop.

The order of the events still differs. AC sorts the folders newest first (`LastWriteTime`). The
page sorts them by name, because a browser folder handle has no dates. On this install every
folder's date comes from the install itself, so AC's order there is arbitrary anyway.

## Step 2: a recorded lap becomes the live AI line

In the game, `AISplineRecorder::onLapCompleted` @ 0x140296760 copies the lap just recorded into
`bestLapSpline`. `getBestLapSpline` hands that line to everyone: the spline locator of every car,
the AI drivers, the keyboard steering help and `Car::postStep`'s track sides. So from the first
recorded lap on, each car's place on the lap is measured on the new line. rustyAC kept the old
line. That is why Task 44's whole-car comparison drifted from the first finished lap on
(`car0.npos`).

The recorder now lives in the physics engine (`PhysicsEngine::ai_recording`), stepped where
`Track::step` steps it. Every lap car 0 finishes publishes the recorder to the track's live line,
`Track::live_ai`. It is an append-only list, so lines handed out earlier stay valid, and
`Track::ai_line()` / `Track::ai()` give the newest. Every reader during a session now uses it: the
spline locator, the AI's lap length and reset, the referee, the replay's track and the autodriver.

| proof | what is checked | result |
|---|---|---|
| `car_oracle multi --scenario ai_solo --track spa --steps 100000 --ai-record --threads 0,8`, and `multi-check` with 0 / 1 / 8 threads | both saved files and every value of the whole car, 2 laps and a pit lane | files IDENTICAL (2,752,176 / 55,408 bytes); cars **IDENTICAL** for 100,000 steps (before: DIFFERENT at step 37,191) |
| the same at magione with the bmw_m3_e30, 75,000 steps | the same | files IDENTICAL (815,080 / 30,568 bytes); cars IDENTICAL |
| `tools/proofs/lib.sh` | the `--ai-record` lines | the cars must now agree too (the old exception is removed) |

With several cars, the game swaps the line inside car 0's step while the other cars step on
worker threads, so which line those cars see in that step depends on timing. rustyAC swaps after
every car has stepped. With one car, as proven here, both ways give the same result.

## Step 3: FormAI, the AI app

`crates/rustyac-gui/src/app_ai.rs` ports `FormAI::FormAI` @ 0x1402dcf80 and `render`
@ 0x1402e1480. It is the 400 x 740 window with its speed graph, the "start recording", "start
pits" and "save cons" buttons, the AI driver's eight spinners and magic switch, "SAVE ai", and the
red debug text. The buttons and spinners become drive jobs (`SetupJob::SetAi`,
`SetupJob::AiRecording`), so a recorded drive replays them.

Where the files go:
- A recording started from the app is saved at the end into
  `<cfg folder>/content/tracks/<track>/<layout>/ai/`. The game saves it into the track's own `ai`
  folder.
- `ai.ini` and `fuel_cons.ini` go into `<cfg folder>/content/cars/<car>/data/`. The game writes
  them into the car's data folder.

| proof | result |
|---|---|
| GUI oracle `app_ai` (36 frames) and `app_ai_driver` (106 frames): pictures, state, `ai.ini` (243 B), `fuel_cons.ini` (36 B), messages | **IDENTICAL** |
| neighbouring sequences `app_setupdebug`, `app_drszones`, `app_keyboard`, `app_x360` | IDENTICAL |

## Step 4: photo mode

- **END:** `Scene::ray_picker_pick` (`crates/rustyac-render/src/scene.rs`) ports
  `RayPicker::pick` @ 0x14044c8c0, `RayPickerTraverser::onNode` @ 0x14044c510 and
  `Collisions::rayMeshIntersect` @ 0x14044cdd0. Within one mesh the first triangle hit counts,
  not the nearest; across meshes the nearest counts. While END is held, the photo window sets the
  depth-of-field focus to that distance and writes it in red at (0, 0), as the game does.
- **Exposure multiplier:** the game starts it from Documents `cfg/exposure.ini`
  `[EXPOSURE] VALUE` (`Sim::Sim`) and writes it back at the end (`Sim::~Sim`). rustyAC reads its
  own cfg folder first, then Documents. It writes to `<cfg folder>/exposure.ini`, never to
  Documents.
- **Clicking Start live:** with `rustyac.exe --track magione --car bmw_m3_e30 --no-race-ini
  --duration 8 --cfg-dir <d> --gui-do "app:photomode;click:140:141;screenshot"`, the physics
  paused after 68 steps (2,662 steps without the click). This needed a fix: `--gui-do` commands
  after an `app:` command waited for an app that did not exist yet, so they did nothing. They now
  run the frame after the apps are made.

| proof | result |
|---|---|
| `RENDER_ORACLE_PICK=<stem> render_oracle compare`: the game's own `RayPicker` on the game's scene against the port's, at 5 screen points a frame; scenes r20_13 (magione chase), r20_06 (spa cockpit), r20_07 (spa chase, sun 40) | 5 of 5 bit-identical in each |
| the same over r21_01 (spa dash, 58 frames) | 290 picks, all bit-identical |
| GUI oracle `app_photo` (115 frames) | IDENTICAL |
| `ray_picker_tests` unit test | pass |
| live: `exposure.ini` with `VALUE=1.35` in the cfg folder | read, then written back as `[EXPOSURE]\r\nVALUE=1.35\r\n` |

## Step 5: online DRS zones, the online static fields, the replay page

- **DRS zones:** acServer reads the track's `data/drs_zones.ini` and sends message 0x53 when a
  client joins. Every recorded tape has one zone (start 0, end 0.001).
  `ACClient::onDRSZoneReceived` @ 0x14004b520 replaces the track's zones
  (`DRSManager::setZones`), with detection 0. In rustyAC, the network session sets
  `Track::server_drs_zones`, and `is_drs_available` uses it ahead of the track's own zones.
- **Online static fields** (`SharedMemoryWriter::writeStatic` @ 0x1401877c0): online,
  `isTimedRace` is the last race session's value. rustyAC always wrote 0 there; that is fixed.
  `hasExtraLap`, `reversedGridPositions` and the pit window come from the client.
- **Replay graphics page:** the status is 1 (replay), `replayTimeMultiplier` is filled in,
  the position is -1, `sessionTimeLeft` is -1, and the last lap comes from the car without splits
  (`GraphicsFrame::in_replay`). rustyac.exe sets these while its replay plays.

| proof | result |
|---|---|
| `car_oracle net` with the game's own 0x53 handler, the track's zones compared every step: 5 tapes x net_watch / net_block / net_drive / net_pit x 3,000 steps | 20 of 20 **IDENTICAL** |
| `sh tools/proofs/net.sh` (the goldens, four ways) | 80 of 80 IDENTICAL |
| `CAR_ORACLE_SHM_ONLINE=1,4,3,8` and `0,0,0,0`, `pit_box_wing` (f2004 + e30), threads 0 / 8 | static page and run IDENTICAL |
| `CAR_ORACLE_SHM_ONLINE=0,2,1,3`, `rc_timed` (the game's `isTimedRace` is 1) | IDENTICAL |
| `CAR_ORACLE_SHM_REPLAY=40:120:0.25` (with online 1,4,3,8), 2,000 steps; `14500:14700:2`, 45,000 steps (after lap 1) | IDENTICAL |

Having the real game join our own acServer was not tried: the rules forbid running acs.exe. What
is proven:
- for DRS, the game's own client code on the recorded server bytes;
- for the shared memory, the game's own `writeStatic` and page update, run with a stand-in client
  and replay manager.

## Step 6: the console variables

`axisMode` is done. The console's lambda @ 0x140196710 accepts only 0 to 2 and sets the scene
camera's mode. `CameraShadowMapped::renderPass` then draws `Camera::renderAxis` @ 0x14020f360
after the opaque pass: three lines of length 1 from the origin. Render oracle `r44c_01` (magione,
`axisMode` 1, 2 and 0, then 3, which is refused) gives a **IDENTICAL** command log and pixels over
7 frames (906 draws, 907 while the axes are on). The case is added to
`tools/proofs/render_list.txt`.

The other console variables go to Task 44d (see "Not done").

## Step 7: write paths

- **TempRun** (Suspensions window): the game's button queues `BrakeSystem::activateTempRunFile`
  @ 0x14028d400 on the physics thread. That opens `TempRuns/<car>@<track>.trun` in the game's
  folder and writes an `i32` 1. After that, every physics step after the brakes
  (`saveTempsRunFame` @ 0x14028e4e0) adds the speed, the four wheels' angular speeds and the four
  brake torques. AC ships without a `TempRuns` folder, so the game writes nothing unless you make
  that folder.
  In rustyAC, a `SetupJob::TempRun` drive job starts and stops it, and the bytes are kept on the
  car. At the end of the run they are written to `<cfg folder>/TempRuns/<car>@<track>.trun`,
  only when that folder exists (as in the game).
- **Driver model save:** it already went to the cfg folder. Its `data_<config>` path stays open
  (Task 44d).

| proof | result |
|---|---|
| `CAR_ORACLE_TEMP_RUN=500:2500 car_oracle multi --scenario ai_solo --track spa --steps 3000 --threads 0,8`, F2004 | file (72,004 B) IDENTICAL, cars IDENTICAL |
| the same with `--cars ferrari_f40` (a car with brake temperatures) | file IDENTICAL, cars IDENTICAL |
| live: F40 at magione, `--gui-do "app:suspension;click:110:770"` | 93,208 bytes written |

## Step 8: the browser Settings leftovers

- **Start Benchmark:** AC's theme sets `race.ini [BENCHMARK] ACTIVE=1` and starts the game. The
  game then plays `content/benchmark/benchmark.acr` with a benchmark HUD and writes an
  `acbench_*.txt` result (`BenchmarkMode::renderHUD` @ 0x14005c0c0, `generateResultString`
  @ 0x14005b370). rustyac.exe can play that replay but has no benchmark mode, so the page keeps
  its notice (Task 44d).
- **`?launcher=0`:** the old key-by-key graphics list is back (29 rows, with "Render scale
  (rustyAC)" last).
- **rustyAC tab icon:** it uses the theme's own tab markup with a white steering-wheel glyph
  (inline SVG), drawn like the theme's 64 px icons. When the theme adds a second icon of its own,
  CSS hides it. A script checked it (one visible icon, with the SVG background). It was not
  looked at in a picture, because the headless window does not show that part of the tab column.
- **Clear cache, Changelog, language** (`tools/web_checks/task44c/general.txt`, run in the
  desktop launcher and in the page): both sides send the same requests (`setlanguage/it`,
  `WINDOW/LANGUAGE=it`, the reloads, "Clearing preview cache"). Two fixes in the page:
  - The changelog was missing. The page now reads `changelog.txt` from the game's folder, and
    the pack includes it.
  - A notice's OK button did nothing; only Esc closed it. OK now closes it.
- **Profile's raw `<l18n…>` filter text:** the theme puts `<l18n>` inside `<option>`, and a
  select shows that as raw text. Desktop (WebView2) shows the same. The real launcher is also
  Chromium (CefGlue), so AC shows it too. No change.

## Step 9: `race_out.json` after the server starts its next race

When the server goes on from a session, `lambda_9cebb0` @ 0x140133f90 keeps a practice's or a
qualifying's results, but returns at once after a race. `RaceManager::shutdown` @ 0x140141320
then writes the session in progress into the slot of its type. So if you leave after the server
has started its next race, the game's `race_out.json` holds that new race (no laps yet, in grid
order), and the race that just finished is lost. rustyAC already did the same (Task 43).

Live check `sh tools/web_checks/task44c/race_loop.sh 430` (Kunos' acServer, races only,
`LOOP_MODE=1`, 1 lap, F2004 cars "a" and "b"): acServer finished three races and wrote three
`_RACE.json` files. Both clients left early in the fourth race. Both clients' files hold one
session, "Race", with 0 laps, every lap count 0, no best laps and the grid order.

## Step 10

Not started (it was only "if time is left"). It moves to Task 44d.

## The quick run

`bash tools/proofs/run_all.sh --quick`. The physics and fresh areas did not run the first time:
`tools/chassis_compare` no longer built because of step 2's new `SimSetup` field (fixed in its own
commit). They were then run again alone (`run_all.sh --quick physics fresh`).

| area | cases | identical | different | time |
|---|---|---|---|---|
| physics | 9 | 9 | 0 | 0m30s |
| fresh | 51 | 51 | 0 | 11m28s |
| sound | 4 | 4 | 0 | 0m38s |
| render | 12 | 12 | 0 | 3m42s |
| gui | 29 | 29 | 0 | 17m07s |
| python | 1 | 1 | 0 | 0m05s |
| replays | 6 | 6 | 0 | 4m00s |
| net | 16 | 16 | 0 | 5m30s |
| browser | 9 | 9 | 0 | 4m36s |
| ai_page | 3 | 3 | 0 | 8m49s |
| shaders | 4 | 4 | 0 | 1m31s |
| launcher | 1 | 1 | 0 | 1m06s |
| dumps | 6 | 6 | 0 | 5m02s |

## New or renamed web-zip files

No files were renamed. The preview pack must gain `changelog.txt` (141,254 bytes) and
`content/specialevents/*/event.ini` (203 files, about 0.8 MB). `re/gold/web/pack` was not remade
here.

## Not done (Task 44d)

1. Special Events order in the page: it sorts by name. AC puts the newest folder first, using
   dates a browser folder handle does not give.
2. AI line with several cars: the game's swap inside car 0's step races the other cars' steps;
   rustyAC swaps after all cars. Not proven with several cars.
3. FormAI: the 3D splines (`renderSplines`), `AUTO_FUEL_CONS`, the `drivetrain.ini`
   `AUTO_SHIFTER` copy and the side splines' indexes; no live click in a window.
4. Photo mode:
   - END was not pressed by script in a live run.
   - There is no depth-of-field pass, and the renderer does not use the exposure multiplier (no
     YEBIS).
   - FOV and roll set before Start do not move the scene camera.
   - In the `--screenshot` path, the photo window shows FOV 0.
5. Online: the graphics page built from the client's leaderboard, laps and time (the net oracle
   has no shared-memory writer); the real game against our own acServer (running it is not
   allowed).
6. Console variables:
   - `lodDebug`'s red text: the HUD oracle would need to stage `CarLodManager::renderHUD`.
   - `editBoxes`: the live `colliders.ini` reload and the wireframe boxes; `ShapeRenderer` is not
     ported.
   - `editCMesh`: the collider mesh drawn in red; the port's renderer does not load it.
   - The four `observe*`: they reload every 2 s of wall-clock time, so there is no repeatable
     oracle.
   - The cockpit exposure: no YEBIS.
7. Driver model `data_<config>` save path (race.ini `MODEL_CONFIG`, which rustyac.exe does not
   use).
8. Start Benchmark (`BenchmarkMode` is not ported); the tab icon was not seen in a picture.
9. Step 10: Telemetry's ride-height channels one step late at the 2 m/s edge; remote cars in
   Setup debug.

## Open questions

- Does the real launcher really leave `<l18n>` raw in the Profile filters? This is inferred from
  the theme's markup and from it being Chromium; no recording of the real launcher's page exists.
- `RACE_OVER_TIME=150` had no visible effect in the race loop: everyone finished, and the
  results came 11 s before the next race. acServer's rule for it is not checked.

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
- **Step 4, photo mode, done and proven:** the developers' "Photo mode" (`PhotoMode`,
  `app_photo`: pictures, state, every frame's camera, exposure, DOF and sun, and the calls it makes:
  free camera, physics pause, screenshots).
- **Step 5 done:** the "Post Process Filter" picker (`app_ppfilter`), the "HDR Settings"
  window (`app_hdr`) and the "Driver Model Utilities" (`app_driver`), all proven.
- **Step 7 (only "if time"):** not started, moved whole to Task 44c (list at the end).
- **Released as v0.44.0.** Next: Task 44c, from "Not done (Task 44c)" at the end of this file.

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
  finished lap on (`car0.npos`); the proof judges the files only. Task 44c.
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
  Task 44c.

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

Not done (Task 44c):
- rustyac.exe does not read its own saved cameras.ini / dash_cam.ini back on the next start (the
  game reads its edited files; rustyAC reads the game's).
- `[AC_APPS] AUTOEXP` in the game's assetto_corsa.ini starts AUTO EXP and the game quits
  (`exit(2)`) once it has saved; rustyac.exe reads the value but does not quit.
- What `ACCameraManager::setMode` does besides switching the view (the game's free camera, track
  cameras) was not traced.

## Step 4: photo mode (`PhotoMode`)

AC's photo mode is a developers' app ("Photo mode", in the developers' apps bar; no key). It is
ported in `crates/rustyac-gui/src/app_photo_mode.rs` and made in rustyac.exe like the game does
(after the GPU profiler, before "DRS Zones").

### How to use it

Open "Photo mode" from the developers' apps. **Start** switches to the free camera (as F7 does)
and pauses the physics (not in a replay); **Start** again goes back to the view you had and
unpauses. The precision spinner sets how big each step is (10^(value-3)). The -/+ buttons: the
exposure multiplier (±0.005), field of view (±0.5 × precision), DOF factor, DOF focus (0..1000),
roll, sun azimuth and zenith (±0.35° × precision; the first press turns on the custom sun and stops
the sun's own animation). While started: the view finder is drawn over the screen, the **middle
mouse button** takes a screenshot each frame it is down (into the same folder as F8's), the
**wheel** over the window changes the field of view, and with the **right button** held the DOF
focus. It writes no files.

### The proof

`GUI_ONLY=app_photo sh tools/proofs/gui.sh` (render oracle sequence `app_photo`): the game's
`PhotoMode` built on a staged scene camera, camera manager, sun animator and graphics manager,
the port's app beside it; every spinner clicked up and down at three precisions, Start, the middle
button for two frames, the wheel with and without the right button, Start again, the wheel while
hidden. `GetAsyncKeyState` answers the script's buttons; `setMode`, `pausePhysics` and
`takeScreen` are written down instead of done.

| what | result |
|---|---|
| command log, pixels (115 frames) | IDENTICAL |
| state and events (8168 lines, 348 events) | IDENTICAL |
| every frame's camera matrix, FOV, DOF, exposure, mode, sun, sun animator, and the calls | IDENTICAL (18456 bytes) |

Checked in rustyac.exe: `--screenshot photo.png --gui-do app:photomode` shows the window.

### Not done (Task 44c)

- **END** (the game's RayPicker: DOF focus = the distance to what is in the middle of the
  screen, written in red) is not ported: rustyAC has no ray picker on the drawn scene.
- rustyAC's renderer has no depth-of-field pass and does not use the exposure multiplier, so the
  DOF and exposure spinners change the values (shown, proven) but not the picture. The custom sun
  and the field of view / roll do change the picture.
- In the game the FOV and roll spinners move the scene camera even before Start; in rustyac.exe
  they only move the free camera (after Start).
- The exposure multiplier's start value (1) is assumed, not read from the game's GraphicsManager.
- The live Start / screenshot path in rustyac.exe was checked by building and by the window
  showing, not by a scripted click in the windowed game.

## Step 5: driver model, HDR, post-processing filter

### The "Post Process Filter" picker (`PostProcessFilterSelector`)

Ported in `crates/rustyac-gui/src/app_pp_filter.rs`. Like the game, rustyac.exe makes it only with
post-processing on (`--post on` or video.ini `[POST_PROCESS] ENABLED=1`: in the game the window
belongs to YEBIS' camera). It is a normal app (not a developers' one).

How to use it: the spinner lists the filters of Documents' `cfg\ppfilters` (rustyAC's own cfg
folder first, else the game's Documents) and then of the game's `system\cfg\ppfilters`, in the
folders' order, with the game's quirks (a name in both folders shows twice; video.ini's `FILTER`
is only looked for among the first N items, N the game folder's count). -/+ pick a filter. At the
end of the session the filter showing is written to video.ini `[POST_PROCESS] FILTER`: the game
writes Documents\Assetto Corsa\cfg\video.ini, **rustyac.exe writes its own cfg\video.ini**.

Waits for YEBIS: picking a filter changes nothing in rustyAC's picture (the game loads the
filter's settings into YEBIS, `loadPPSet`, and rebuilds its render targets). rustyac.exe drops
the request.

Proof `GUI_ONLY=app_ppfilter sh tools/proofs/gui.sh` (one Documents filter `my_look` before the
game's ten, so the start index quirk shows; -/+ clicked past both ends; the game's end):

| what | result |
|---|---|
| command log, pixels (55 frames) | IDENTICAL |
| state and events (1585 lines, 100 events) | IDENTICAL |
| every filter asked for (`loadConfigurationFile`, the start one included) | IDENTICAL (424 bytes) |
| cfg/video.ini written at the end | IDENTICAL (513 bytes) |

Checked in rustyac.exe: `--post on --screenshot ppf.png --gui-do app:ppfilter` shows the window on
"default".

### The "HDR Settings" window (`FormHDR`)

Ported in `crates/rustyac-gui/src/app_hdr.rs`, a developers' app. Like the game, rustyac.exe
makes it only with post-processing on (it belongs to YEBIS' camera).

How to use it: the graph shows YEBIS' auto exposure (white), the filter's minimum (yellow) and
maximum (red); "Disable Limits" / "Enable Limits" switches the limits; the OnBoard / OutBoard EXP
spinners change car 0's exposures (0..100); "PP Target (not saved)" changes the filter's target
(0.00..1.00); "Save in car.ini" writes `[GRAPHICS] ONBOARD_EXPOSURE` / `OUTBOARD_EXPOSURE` and
shows "SAVED" for 3 s of game time. It is hidden when the car has a `data.acd`, as in the game.
The game writes `content\cars\<car>\data\car.ini`; **rustyac.exe writes its own cfg's
`content\cars\<car>\data\car.ini`** (the Camera editor's mapping).

Waits for YEBIS: rustyAC's picture has no auto exposure, no per-car exposure and no filter
target, so in rustyac.exe the graph and label stay 0 and the edits change nothing on screen (the
saved car.ini is the game's). The filter's min / max / target start at 0 there (a `shortcut:`
comment in `gui_apps.rs::camera_editor_link`).

Proof `GUI_ONLY=app_hdr sh tools/proofs/gui.sh` (an auto exposure moving every frame; limits
off, on, off; both exposures and the target clicked both ways; Save; the 3 s running out):

| what | result |
|---|---|
| command log, pixels (84 frames) | IDENTICAL |
| state and events (3940 lines, 76 events) | IDENTICAL |
| car 0's exposures, the limits and the target every frame | IDENTICAL (15036 bytes) |
| saved car.ini (ONBOARD 37, OUTBOARD 44) | IDENTICAL (1544 bytes) |

Checked in rustyac.exe: `--post on --screenshot hdr.png --gui-do app:hdr` shows the window.

### The "Driver Model Utilities" (`FormDriverModel`)

Ported in `crates/rustyac-gui/src/app_driver_model.rs`, a developers' app (auto-hiding, no
background until the mouse is over it). It works on car 0's 3D driver (the high-detail one).

How to use it: "G Force" and "G Direction X/Y/Z" (Negative / No Force / Positive) push a
G force on the driver's head; from the first change the driver's own animation is locked, as in
the game. The red button reads "Start": the first click loads the car's `data/driver3d.ini`
`[HEAD_MOVEMENT]` into the right column (Max G Force, Roll / Yaw / Pitch Max Deg, Filter; Filter
15 without that section) and the button becomes "Save Ini"; from then on those spinners change
the driver's head movement and "Save Ini" writes the five keys. The button is hidden when the car
has no loose driver3d.ini (packed cars), as in the game. The game writes
`content\cars\<car>\data\driver3d.ini` (or `data_<config>`); **rustyac.exe writes its own cfg's
`content\cars\<car>\data\driver3d.ini`**.

The game's quirk kept: the Filter spinner stores `value * 0.01 / 60` while the driver's own load
stores `FILTER * 60`, so moving it makes the head follow far more slowly.

Proof `GUI_ONLY=app_driver sh tools/proofs/gui.sh` (G spinners before Start, Start, every head
spinner both ways, the G spinners again, Save Ini; the driver model a stand-in block with
cardata's values, `updateHeadMovement` written down with its arguments):

| what | result |
|---|---|
| command log, pixels (71 frames) | IDENTICAL |
| state and events (6728 lines, 125 events) | IDENTICAL |
| the driver's lock, head movement and every `updateHeadMovement(dt, dt, accG)` call (69 calls) | IDENTICAL (7067 bytes) |
| saved driver3d.ini (FILTER 0.2, MAX_G 2.3, ROLL -11, PITCH -11, YAW 18) | IDENTICAL (387 bytes) |

In rustyac.exe the window's writes and calls go into car 0's `driver_hr` after the GUI's render
(`crates/rustyac-game/src/render/ac.rs`), so the head moves from the next frame, as in the game.
Checked: `--car ks_ferrari_f2004 --screenshot drv.png --gui-do app:drivermodel` shows the window.
Not done: the car's `configName` is always empty in rustyac.exe (no `data_<config>` save path);
the lock is set again every frame (a `shortcut:` comment), which differs from the game only for
a car whose `LOCK_VIRTUAL_STEER` clears it on board.

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

## Release check (v0.44.0)

`bash tools/proofs/run_all.sh --quick` (rule 14). The first run found the browser build broken by
step 1 (`ai_record.rs` uses the Windows-only physics thread; now `#[cfg(windows)]`); browser and
ai_page were run again after the fix.

| area | cases | identical | different |
|---|---|---|---|
| physics | 9 | 9 | 0 |
| fresh | 51 | 51 | 0 |
| sound | 4 | 4 | 0 |
| render | 12 | 12 | 0 |
| gui (the new apps' sequences included) | 29 | 29 | 0 |
| python | 1 | 1 | 0 |
| replays (the AI line recording included) | 6 | 6 | 0 |
| net | 16 | 16 | 0 |
| browser (rerun) | 9 | 9 | 0 |
| ai_page (rerun) | 3 | 3 | 0 |
| shaders | 4 | 4 | 0 |
| launcher | 1 | 1 | 0 |
| dumps | 6 | 6 | 0 |

## Not done (Task 44c)

Collected from the steps above, plus step 7:

1. AI line: the kept lap does not become the live AI line at once (the game's `bestLapSpline` is
   the track's line); the FormAI window (buttons, hints editor, drawn splines) is not ported.
2. DRS: the zones a server sends (0x53) are decoded, not acted on.
3. Camera editor: rustyac.exe does not read back its own saved cameras.ini / dash_cam.ini;
   `[AC_APPS] AUTOEXP`'s quit after the save; `ACCameraManager::setMode`'s other effects.
4. Photo mode: END (ray picker), no DOF pass, exposure multiplier unused, FOV/roll before Start.
5. Waits for YEBIS (rustyAC has no YEBIS): loading a post-processing filter, the per-car
   exposures, the auto exposure and its limits and target (rustyac.exe shows 0), the exposure
   multiplier, depth of field. The windows edit and save these values like the game; the picture
   does not change.
6. Driver model: no `data_<config>` save path (configName always empty in rustyac.exe).
7. Step 7, all of it: the console variables `axisMode`, `lodDebug`'s red text, `editBoxes` /
   `editCMesh`, the four `observe*` live reloads, the cockpit exposure; `race_out.json` after an
   online server's next race.

## Open questions

- The exposure multiplier's start value: rustyAC assumes 1; docs/map/yebis.md says the game takes
  it from Documents `cfg/exposure.ini [EXPOSURE] VALUE` (not checked against the game).
- The whole-car compare with `--ai-record` after the first lap: expected to agree again once the
  kept lap is the live line (item 1), not proven.
