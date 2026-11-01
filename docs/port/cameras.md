# AC's cameras (Task 46)

## Task 46b: the rest of AC's cameras

### Resume here

- Step 1 + 3 (game side) + 2 (helicopter) ported: `crates/rustyac-game/src/render/onboard_free.rs`
  (`OnboardFreeCamera` F5, `CinematicCamera`, `HelicopterCamera`); the game's
  `CameraOnBoardFree::update`, `CinematicFreeCamera`'s moves and fov keys and
  `CameraHelicopter::update` against them frame by frame (`orbit_camera.tape`, new in
  `car_oracle start-camera-micro`): IDENTICAL on seeds 1 and 2 (60200 lines each).
- Desktop: F5 through the fade (`--focus-script onboard@3`: `ONBOARD FREE CAMERA`, the car seen
  from theta 75, phi -45, radius 5), the mouse wheel's radius, the ESC menu's and the results'
  F5 (`SetIntroMode`, `SetCameraMode`), the pit crew's F5 (locked, theta/phi forced), Python's
  `setCameraMode` 0-7 through one `setMode` (helicopter 4), F7 as the cinematic camera when
  `video_editing.ini [CINEMATIC_CAMERAS] ACTIVE` (this PC has no such file).
- Page (Full mode): F5 through the fade, the left button's drag, the wheel, F1 back
  (`tools/web_checks/task46/f5.sh`: HUD hook `camera` "chase" → "onboard free" → "chase");
  the results' and the pit crew's F5, F7 as the cinematic camera when `video_editing.ini` says
  so. One mode-switch for both (`onboard_free::set_mode`, `f1`, `f6`). The helicopter is
  Python's only, and the page runs no Python. Shortcut: the page cannot move the mouse, so the
  cinematic look's wrap at the desktop's edges never happens there.
- Step 3b: the track cameras in a replay follow its speed (slow motion, fast forward, rewind,
  pause): the desktop times `dt` by the rig's `timeMult` in replay mode as `CameraTrack::update`
  does (the tape's `in replay` / `replay speed` columns already covered the sum). The page has no
  AC replay.
- Step 4: the last camera remembered as `ACCameraManager` does: `[LAST_CAMERA] MODE` and
  `DRIVABLE_MODE` read with `camera_manager.ini` (rustyAC's own copy, else Documents'), kept by
  every switch as `setMode`'s end keeps them (the cockpit unless the random camera chose it, the
  drivable views), taken back at the ESC menu's first Drive (`loadPreviousSessionUserCamera`)
  and written at the end into rustyAC's own cfg folder through the write guard (the game's
  destructor writes Documents'). `tools/web_checks/task46b/last_camera.sh` (new `--gui-do drive`,
  the ESC menu's Drive button): Documents 2 3 → bumper, written 2 3; the next run with
  Documents 0 0 → bumper again (the own copy wins); Documents 0 3 → cockpit, written 0 3.
  The page writes no files: it takes the launcher's Documents' last camera.
- Step 5: the windows' camera calls as the game makes them. The replay's camera spinner now
  lists the track's camera sets after the cockpit and the five drivable views (`getCameraSetName`)
  and picks a set (`setCurrentCameraSet`, it was ignored); the Camera editor's F1 / F6 buttons
  go through `setMode(mode, false, false)` (which cycles, as F1 does) and then
  `setDrivableCarIndex` / `setCameraCarIndex` (the old code kept the mode); the photo mode's
  Start and back are `setMode(eFree)` and `setMode(the mode it found)`; `setMode(eCar)` and
  `setMode(eFree)` set the near plane 0.05; F1 from the free, start or F5 camera goes to the
  last driving camera from any mode. The random tape (`random_micro`) now also drives the
  spinner, the editor's buttons, the photo mode and `loadPreviousSessionUserCamera` on the same
  manager, with a made-up `[LAST_CAMERA]`, and checks `persistanceCameraMode` on every line:
  seeds 1 and 2 IDENTICAL (60,120 lines on seed 2: 2,665 spins, 2,832 editor buttons, 827
  photo starts, 412 backs, 1,510 loads). The page has no apps: only the spinner's set and the
  mode kept apply there (no AC replay either).
- Step 6: Python's camera calls proven on the game's code. `ac.setCameraMode` (0x14012d7d0)
  is `randomMode = 0; setMode(v, false, false)`, or for 7 `setMode(eTrack)` then `randomMode =
  1`: the random tape now calls it with 0..7 on the same manager (seed 2: 1,477 calls, every
  mode), the port runs it as the desktop does (`camera.random &= mode == 7`, then
  `onboard_free::set_mode`; before this the desktop kept the random camera's flag, so 0 from a
  drivable view the random camera chose went to the last driving camera instead of cycling).
  `ac.freeCamera*` move the scene camera (`CameraMouseControlBase::getCamera`) with
  `Camera::moveForward` / `moveRight` / `moveUpWorld` / `rotatePitch` / `rotateHeading` /
  `rotateRoll`: the free tape now mixes them with the free camera's steps (seed 2: 14,993 calls),
  checked on the port's `MouseCamera`. Both seeds 1 and 2: all six tapes IDENTICAL.
  `ac.getCameraMode` / `ac.isCameraOnBoard` now read this frame's camera manager mode and dash
  view (they kept the mode Python last asked for, and the dash flag never changed); the free
  camera counts as `eFree` (6) for them and the GUI. Shortcut kept: right after its own
  `setCameraMode` an app reads the mode it asked until the next frame. The page runs no Python.
- Step 7a, F2 / F3 on a track without camera sets: the game has no fallback. `CameraTrack`'s
  ctor adds a set only for each `cameras*.ini` it loads (no default set), `setCurrentCameraSet`
  refuses any index of an empty list, and `nextSet` 0x1400cbc90 then reads the first set's name
  through the empty vector's null pointer: acs.exe crashes on F3 a second time or on F2's first
  track draw. `CameraTrack::update` itself does nothing without sets, so after the first F3 the
  scene camera stays where it was. rustyAC does that last part (desktop and page now keep the
  last frame instead of drawing the driving camera) and skips the crash; the `shortcut:` is gone.
  No shipped track lacks `cameras.ini`.
- Step 7b, the fade colour: `ACCameraManager`'s ctor reads `camera_manager.ini [FADE] COLOR`
  (when the section is there) into `fadeColor`, and the fade lambda 0x140032770 draws its quad in
  that colour; nothing else writes it. The port now reads it with the rest of the file (own cfg
  else Documents) and every fade (F2-F6, the crew's, the race rules', the menus') draws it. Unit
  test `last_camera`; desktop run with Documents `COLOR=1,0,0`, F3 at 3 s: the screen at 3.3 s is
  red-tinted (pixels (171,51,30), (166,38,41), (191,40,34)). Black comes from the ctor's default
  and every shipped `camera_manager.ini`'s `0,0,0`.
- Next: proofs, report, release.

## Resume here

- Step 0 done: pit_mouse crash fixed; the page's first start compared with a new desktop player;
  606 old kept results (36 GB) moved to the server, SHA-256 matched (below).
- Step 1 done: the audit table below, sent to the user and the split agreed.
- Step 2a done: the cockpit, the five drivable views and F6 against the game frame by frame
  (`driving_camera.tape`), with the g forces, shake and look keys in the same code; IDENTICAL.
- Step 2b done on the desktop: the camera settings files, the look-back points (collider box
  IDENTICAL on all 137 cars), the shake's random blend, the look keys and Ctrl+Numpad FOV keys.
  The game has no seat keys.
- Step 2b done in the page too (Full mode): the player's camera files from the launcher's
  Documents, the look keys and Ctrl+Numpad FOV keys; settings line IDENTICAL to the desktop's.
- Step 2c done: the race rules' fades (restart, next session, race end, Time Attack / Drift /
  Drag) in AC's picture, desktop and page.
- Step 2d done: F3, the track's TV cameras and their sets: every track's sets as the game loads
  them and `CameraTrack::update` frame by frame IDENTICAL (`track_camera.tape`); F3 through the
  fade on the desktop and in the page.
- Step 2e done: F2, the random camera: `ACCameraManager::update` / `stepCameraRandom` and the
  keys around it IDENTICAL against the game (`random_camera.tape`); desktop and page.
- Quick proofs done: `run_all.sh --quick` 167 of 167 cases IDENTICAL (below). Released as
  v0.46.0. Next: Task 46b.

## Step 0: housekeeping

### The GUI pit_mouse crash

The GUI oracle made the escape menu a plain `Control` (0x178 bytes, `gui_ac_hud.rs`); the pit
sequence reads `escMenu+0x1a8` (the menu's setup screen) and puts in its own only when it reads 0.
That word was past the block, so heap garbage: the game followed it into
`SetupScreen::isFixedFuel`. The block is now 0x400 zeroed bytes. GUI `pit_mouse` 10 runs: command
log and pixels IDENTICAL in all 74 frames every time, no crash. The physics side, `car_oracle multi`
on the five `pit_mouse` lines of `multi_fresh.txt` at 0, 1 and 8 threads, 10 runs each: 50 of 50
IDENTICAL at every thread count, no crash.

### A new player's first start: page against desktop

Task 45c's open question (g-force 1.4 against 1.0, AI level 98 against 100) was not a page
difference: the desktop launcher's `--player-cfg` (on by default) copies this PC's own
`Documents\Assetto Corsa\cfg` into the new player's folder first. A new AC player has none; with
`--player-cfg none` the desktop writes 1.4 and 98, the page's values. The page needs no change for
that. Task 45c's question had it backwards: the page was already right, the desktop's 1.0 and 100
were this PC's own settings.

Found on the way: after `ac://home` the page let the theme's old document run on until the frame
reloaded (one more career start and two more Drive restores, all written to `race.ini`); the
desktop's WebView runs none of it. The page now answers the leaving document's requests empty and
passes none to the launcher (`web/theme_glue.js`, `web/launcher.js`).

Proof: `tools/web_checks/task46/first_run.py` (desktop `first_desk.txt`, page `first.txt`, both
from an empty folder, language picked, Drive opened in both documents):

| | desktop | page |
|---|---|---|
| files in the player's folder | 58 | 58 |
| `cfg/` files that differ | `video.ini` (page: its window size, 800x600 windowed, by design), `race.ini` (one trailing empty line) | |
| other files that differ | `launcherdata/cache_*.json`: the AC folder's path (the page's is `C:/assettocorsa`) and special events' listing order | |
| requests (counted) | 3757 | 3799 |

The requests left that differ: the page opens Drive by itself when the theme starts, once more than
the desktop script clicks it (the Drive restore writes the same values again); the theme's career
start finishes in the page's first document and not in the desktop's (timing: on the desktop the
language click lands while the career is still loading, "Career reloading language"); and spelling only
(`getCfg`/`getcfg`, `getvideomodes`'s slash, `generatecss` times).

### Old kept results to the server

`tools/proofs/archive_kept.sh` (dry run) found 817 kept folders in `re/gold/fresh/rec/`, 216 of
them today's keys (5 more not kept yet: the five cases whose recording failed in the last run).
`--move` sent the other 606 (36236 MB, 2295 files) to
`10.0.0.140:/storage/rustyac-archive/kept/2026-11-01`; SHA-256 matched for all 2295 files there
before anything was removed here. `rec/` 50 GB → 15 GB, C: 23 GB → 58 GB free. A dry run after:
0 old folders, 216 current keys.

## Step 1: the audit

Proven = a game-against-port compare covers it. Before "missing", both the repo (by feature) and
`docs/atlas/camera.md` (by the game's class and function names) were searched.

| Camera / feature | Keys | Status | Proof or gap |
|---|---|---|---|
| Start camera (`CameraModeStart`) | session start | proven | `car_oracle start-camera-micro` seeds 7 and 34 → `start_camera_check` |
| F7 free camera (`CameraMouseControl`) | F7 (`ALLOW_FREE_CAMERA`) | proven (numbers) | the same check; key and mode switch live only |
| F4 / focus other cars, fade | F4, `--focus` | proven | render oracle sequences 10 and 18 |
| Rear mirror, virtual mirror | F11 | proven (pictures) | `renderer_scene.md` scene rows |
| Camera editor, chase-cam editor, photo mode, eyes app | app windows | proven (GUI) | GUI sequences; the camera they move: Task 46b |
| Replay camera spinner | replay bar | proven (GUI) | the camera it picks: Task 46b |
| F1 cockpit (`CameraOnBoard`) | F1 | proven (step 2a) | `driving_camera.tape` → `start_camera_check` |
| F1 drivable views (`CameraDrivableManager`), `chase_cam.ini` | F1, C | proven (step 2a) | the same; `chase_cam.ini` read as before |
| F6 car cameras (`CameraCarManager`) | F6 | proven (step 2a) | the same |
| Python `setCameraMode` 1/2/6, `freeCamera*` | apps | done, not proven | |
| G-force head movement, shake (`CameraOnBoard::update`) | | proven (2a, 2b) | math frame by frame; `camera_onboard.ini` / `camera_manager.ini` / car `SHAKE_MUL` read (2b), page settings line IDENTICAL to the desktop's |
| Look left / right / back (`lookLeftRight`, `lookBack`, `updateLook`, chase look) | `[GLANCELEFT]`/`[GLANCERIGHT]`/`[GLANCEBACK]`, wheel buttons | proven (2a, 2b) | look-back points: collider boxes IDENTICAL on 137 cars; keys desktop and page |
| FOV (`setPlayerFov`), seat position | Ctrl+Numpad +/- | done (2b) | ±0.5, refused at ≥125 / ≤1, desktop and page; the game has no seat keys (Onboard settings app only) |
| Fade-in (`fadeIn`) | F4, menus, race rules | done | F4's fade proven (`focus.rs`, sequence 18); the menus' fades were already drawn (`GameGui::post_gui`, Task 30; the audit's "does nothing" was the desktop's action loop, which leaves them to the GUI); the race rules' fades now too (Step 2c) |
| F3 track cameras and sets (`CameraTrack`) | F3 | done + proven (2d) | desktop and page |
| F2 random camera (`stepCameraRandom`) | F2 | done + proven (2e) | desktop and page |
| F5 onboard free (`CameraOnBoardFree`) | F5 | missing → 46b | |
| Helicopter (`CameraHelicopter`) | | missing → 46b | |
| Cinematic free camera (`video_editing.ini`) | replay | missing → 46b | |
| Last camera (`loadPreviousSessionUserCamera`) | | missing → 46b | |
| TrackIR | | missing → 47 | |
| Page (Full mode) | F1, C, F2, F3, F4, F6, F7 | as desktop | F5, other cars' views missing |

## Step 2a: the cameras that drive with the car, frame by frame

`car_oracle start-camera-micro` now writes a third tape, `driving_camera.tape`: the game's own
`CameraOnBoard::update`, `CameraDrivableManager::update` and `CameraCarManager::update` on a
made-up car 0 (its 3D matrix from the game's `CarAvatar::makeBodyMatrix`, the body node set as
`CarAvatar::update` does), a new car and new cameras every 300 frames, F1 at frames 100 and 200
(`nextMode`, back to the cockpit after dash) or F6 (`nextCamera`). Each segment turns on, or not:
the g forces (`gForceMix`, `gForceLagMix`, `accGXMul`, `accGZMul`, `chaseCamFilter`), the shake
(`shakeMul`, `shakeFreq`, `shakeScale`, `stillCamera`, `rand()` from seed 1), the look keys (left,
right, both = back, bit 9 = back; `headRotation*`, `rotation*`, the views' own yaw) and
`IS_WORLD_ALIGNED`. Speeds under 2 km/h, upside-down cars, dt 0 and long frames are in it.

`start_camera_check <start> <free> <driving>` plays it through the port's `DrivingCamera`:

| seed | frames | matrix, field of view, near plane |
|---|---|---|
| 7 (20 000 steps) | 20 067 lines | IDENTICAL |
| 1, 2, 3 (120 000 steps each) | 120 400 lines each | IDENTICAL |

Every view saw look left/right, look back and world-aligned frames (counted from the tapes).
`tools/proofs/fresh.sh` has it as the part `cameras` (seeds 1-3, 60 000 steps; `--quick`: seed 1).

What the port changed to get there (`crates/rustyac-game/src/render/scene.rs`):
- the cockpit's eye is `DRIVEREYES` through the 3D model's matrix; the head's g-force offset, the
  shake (`SignalGenerator3D<SinSignalGenerator>`: three sines stepped in whole milliseconds by the
  last frame's frequency, the CRT's `sinf`) and `lookLeftRight` / `lookBack` are ported;
- the chase camera has no "first frame" snap of its own any more (the game keeps its offset across
  modes and snaps on distance or under 2 km/h); its look keys turn it about the car, look back puts
  it 2 m further back, both without the pitch, as in the game; `createTarget` is the exact one;
- a car camera index past the last goes to 0, and the near plane is the last mode's (the game's
  `CameraCarManager` never sets it);
- the machine adds the shake up first and the g-force offset after (the pseudo-C said otherwise).

Not compared: the far plane and the exposure the cameras set (the renderer has its own).

## Step 2b: what the cameras read, and their keys (desktop)

- **Settings** (`DrivingCamera::read_settings`, `crates/rustyac-game/src/render/scene.rs`): the
  reads of `CameraOnBoard::CameraOnBoard` (0x1400c84e0) and `CameraDrivableManager::
  CameraDrivableManager` (0x1400c4a80): `cfg/camera_onboard.ini` (`[GFORCES] LAG`, `[ROTATION]
  HEAD_MAX_DEGREES / SPEED`, `[SHAKE] FREQ / RANDOM`, `[MODE] IS_WORLD_ALIGNED / FOV`),
  `cfg/camera_manager.ini` (`[SHAKE] GFORCEX/Y/Z`, `SHAKEMULT`), the game's
  `system/cfg/camera_drivable.ini` (`[ROTATION] MAX_DEGREES / SPEED`, the speed then overwritten
  by the cockpit file's, as in the game), each car's `car.ini [GRAPHICS] SHAKE_MUL` (the last car
  wins, 0 is 1). rustyac.exe takes the player's files from its own cfg, else AC's Documents.
- **Look-back points**: the middle of the top of the back of the car's collider box plus
  `ON_BOARD_LOOKBACK_OFFSET` / `DRIVABLE_LOOKBACK_OFFSET`. The box is
  `ModelBoundaries::computeAAModelBoundaries` (0x140218d60) of `collider.kn5`, ported as
  `rustyac_physics::car::colliders::model_boundaries`. At that point the game has no world
  matrices yet (the draw makes them), so every mesh counts with the identity: the file's node
  matrices do nothing. The first try used them and 32 of 137 cars differed. New
  `render_oracle boundaries` runs the game's own `KN5IO::load` and the function on every car of
  the install: **IDENTICAL, 137 of 137** (case `cameras boundaries` of the `cameras` part).
- **Shake's random blend** (`SignalGenerator3D::randomBlend`, `RANDOM`): the oracle's tape now
  draws it per segment (0, 0.5, 1 or any); `cameras` seeds 1-3 and 7: IDENTICAL.
- **Look keys**: `KeyboardCarControl::getAction` (0x14010f820) and `DICarControl::getAction` with
  `[GLANCELEFT]`, `[GLANCERIGHT]`, `[GLANCEBACK]` of `controls.ini` (the pad had them already).
  The input thread asks for actions 0, 1, 9 in the step it asks for the headlights, as
  `Car::getPhysicsState` does, and the camera takes them as `actionsState` bits 0, 1, 9 (the
  player's car only). Test `glance_keys`.
- **FOV keys**: `Sim::onKeyDown` Ctrl+Numpad + / - in the cockpit or the dash view: 0.5 a press,
  not past 125 or under 1, message "FOV: %.1f" / "On Board Camera"; `CameraOnBoard` has one
  field of view, so every car takes it. Plain Numpad +/- is force feedback gain, Shift+Ctrl is
  saturation (not cameras).
- **Seat keys**: acs.exe has none; the seat moves with the Onboard settings app
  (`DriverEyesPositioner`), ported and proven in Task 32.
- Not proven against the game: the ini reads themselves (by the game's names and order, not run
  through the game's constructors, which want a whole `Sim`), and the keys' path from the
  keyboard to the camera (the camera's answer to the bits is proven by the tape).

### Step 2b in the page

- The page's drive from AC's launcher now puts the player's `cfg/camera_onboard.ini` and
  `cfg/camera_manager.ini` (the launcher's Documents) where the game reads them
  (`web/app.js` `driveFromLauncher`); the camera reads them once Full mode has set its Documents
  folder (`web.rs`), with the field of view of `camera_onboard.ini` (`CarShape::
  read_documents_fov`, now through the content reader). A drive from the page's own menu has no
  Documents: the game's constructor values, as the game without the files.
- The page's look keys go the desktop's way (`WebSource::headlights` asks for actions 0, 1, 9;
  the glance keys count as keys the layout uses, so the page passes them on). Ctrl+Numpad +/- in
  Full mode's key handler (`full.rs`).
- The HUD hook (`window.rustyac.hud`) has `camera_settings` (the line rustyac.exe prints as
  `camera settings:`), `look`, `onboard_fov` and `head`.
- Proof, `tools/web_checks/task46/cams.sh` (Full mode, AC's launcher Start, AC's `Default.ini`
  controls preset loaded, real keys over CDP): F1 five times: chase, chase 2, bonnet, bumper,
  dash, cockpit; Ctrl+Numpad +, +, -: field of view 56 (the file's) to 56.5; Q held: look 1,
  head +1.0471799 (60 degrees), released 0; W held: look 2, head -1.0471799. Then
  `cams_desk.sh`: rustyac.exe with the same Documents files, car and track: settings line
  **IDENTICAL** to the page's (`mix [-0.0028000001, ...] ... shake 2.8 freq [150.0, 150.0,
  100.0] ... rotation 90.0/20.0`).

## Step 2c: the fades

The game has one fade (`ACCameraManager::fadeMode`), started by `fadeIn` 0x140033690 from 16
places and drawn by the camera manager's post-GUI handler (the lambda 0x140032770: black over
`fadeTotalTime`, the jobs hung on it at the black frame, back over the same time, the quad in
`camera_manager.ini [FADE] COLOR`). Before this step AC's picture in rustyAC drew three of its
starters: F4 (`focus.rs`), the pit crew's commit and the menus' (`GameGui::post_gui`). The race
rules' fades (`RaceManager` restart 1 s, skip / player to pit / race end 2.5 s, Time Attack and
Drift ends 2.5 s, Drag's next match 1 s) were counted by the referee (its jobs run at the black
frame, proven with the race rules in Task 28) but shown only by rustyAC's own HUD as a black
rectangle.

Now `RefereeHud::fade_total` carries the running fade's length and `AcRenderer` starts the
proven overlay (`gl::fade_overlay`) on the frame the referee's fade appears, at `fadeTime` 0 as
the game's post-GUI pass draws it on the frame of `fadeIn`, then runs it on the drawn frames.
Desktop and page draw through the same code (Full mode's frame info carries the referee's).

Run (`re/scratch/task46/fade/run.txt`: `--sessions p10,q5 --restart-session-at 4
--next-session-at 12 --windowed`): `camera fade: 1 s at frame 146`, over at frame 266 (120
frames at 60 FPS, 2 s: black and back); `camera fade: 2.5 s at frame 626`, over at frame 925
(299 frames, 5 s).

Not proven against the game frame by frame: which drawn frame a referee fade starts on (the
referee runs on the physics thread; the game starts it inside its own update). The overlay
itself is the proven one; the lengths are the referee's.

## Step 2d: F3, the track's cameras

`crates/rustyac-game/src/render/track_camera.rs` ports `CameraTrack` (ctor 0x1400cab20,
`loadSet` 0x1400cb670, `nextSet` 0x1400cbc90, `update` 0x1400cbe30) and `CameraManager`
(`load` 0x1401f48e0, `initCamerasDistanceRanges` 0x1401f48b0, `getActiveCamera` 0x1401f4820):
the sets `data/cameras.ini`, `cameras_1.ini` ... of the track (or layout), each camera's
matrix, fields of view, in/out points, planes, exposures, depth of field, shadow ranges, path
(`SPLINE` csv, turned by `SPLINE_ROTATION` as `loadSet` does) and its distances to the AI line;
`system/cfg/camera_track.ini` (distance scale, upward offset, wobble). Per frame: the camera
whose in/out range holds the focused car's place along the lap, aimed at the steering wheel's
place in the car (`orgSteerMatrix`) plus the upward offset, zooming with the distance, wobbling
with the car's speed, or riding its path for `SPLINE_ANIMATION_LENGTH` seconds.

Proof: `car_oracle start-camera-micro` now also writes `track_camera.tape`: for every track and
layout with a `data/cameras.ini` (58 of them), the game's own `CameraManager` reads every set
(its cameras' fields and the paths' points after `loadSet`'s turn are on the tape), then the
game's `CameraTrack::update` runs on a made-up car moving along a made-up line, with random
pauses (still camera) and F3 presses (`nextSet`), with the speed and replay slow motion.
`start_camera_check` reads the same sets with the port and runs the port's update on the same
inputs. `FRESH_PARTS=cameras`, seeds 1 to 3:

| seed | track cameras |
|---|---|
| 1 | IDENTICAL (3243 cameras of 183 sets, 59972 frames) |
| 2 | IDENTICAL (3243 cameras of 183 sets, 59972 frames) |
| 3 | IDENTICAL (3243 cameras of 183 sets, 59972 frames) |

(First try: every camera with a `SPLINE_ROTATION` differed: the oracle had not turned the paths.
`ucomiss; je` in `loadSet`: a rotation that is not a number is no turn; the port does the same.)

Keys: F3 (`Sim::onKeyDown` 0x72) goes through the camera fade like F4 (`registerToFadeInEvent`):
at the black frame `setMode(eTrack)`; F3 while the track's cameras show moves to the next set.
F1 and F6 leave them (F1 to the view driven with before, `lastDrivingMode`). The renderer takes
the camera's far plane, exposures and shadow ranges in track mode; the depth of field is not
drawn by the port (no camera draws it). `currentGlobalCameraIndex` is 6 + the set, as the game's.

Desktop: `rustyac.exe --car bmw_m3_e30 --track magione --camera track --autodrive --no-race-ini
--duration 10 --focus-script track@5 --bench-shot f3.png`: `TRACK CAMERAS: set 1 TV 2`, a TV
camera filming the car. `--camera track` starts on them, `track@<s>` in `--focus-script` is F3.

Page (Full mode), `tools/web_checks/task46/cams.sh` (the HUD hook now gives `track_camera`, the
name of the camera shown): F3 → `track 0`, camera `T1`; F3 again → `track 1`, camera `1`; F1 →
`cockpit`. The page loads the same sets from its folder (the track's `data` and `ai` and
`system/cfg` were already among its files: no new files).

Not proven end to end against the game: the inputs the desktop hands the camera (the car's model
matrix is 2a's proven one; the steering wheel's place, the place along the lap and the speed are
the port's own, proven elsewhere). In a replay the track camera follows the replay's speed
(Task 46b: `CameraTrack::update` times `dt` by `ReplayManager::timeMult` in replay mode; the
desktop's call does it, the same sum the checker does on the tape's speeds).

## Step 2e: F2, the random camera

F2 (`Sim::onKeyDown` 0x71) fades like F3, at the black frame `setMode(eTrack)` then
`randomMode` on. From then `ACCameraManager::update` 0x1400345e0 counts the frame times; past
`randomCameraTime` `stepCameraRandom` 0x140034300 draws a mode by `system/cfg/random_camera.ini`'s
probabilities (AC: cockpit 10, car 15, drivable 5, track 70; 5 to 10 s), one of that mode's
cameras other than the one showing (the same again: the next), `setMode(mode, false, true)`, and
the next time. Every draw is the game's `rand()`; the port draws them from the cockpit shake's
generator (the game has one). F1, F3, F6 from another mode and F7 end it (`setMode`); F6 in car
mode keeps it.

Proof: `car_oracle start-camera-micro` now also writes `random_camera.tape`: a hand-made
`ACCameraManager` (its ctor's random part, `setMode` 0x1400340d0, `update`), groups of 500
events of random keys (F1, F2, F3, F6 when the car has cameras) and frame times; the port's
`start_camera_check` plays the same events and compares mode, cameras, times and `rand()` after
each one.

| seed | random camera |
|---|---|
| 1 | IDENTICAL (120 groups, 60120 lines of the game) |
| 2 | IDENTICAL (120 groups, 60120 lines of the game) |
| 3 | IDENTICAL (120 groups, 60120 lines of the game) |

(First tries: the oracle seeded the CRT's `srand`, but the oracle answers `rand()` itself;
and a drivable view drawn as a wider number than the game's `setCurrentCamera` takes.)

Desktop: `rustyac.exe --car bmw_m3_e30 --track magione --autodrive --no-race-ini --duration 45
--focus-script random@3`: `RANDOM CAMERA`, then `car 4`, `track 1`, `track 3`, `track 1`.
`random@<s>` in `--focus-script` is F2.

Page (Full mode), `tools/web_checks/task46/cams.sh`: after F2, six HUD reads 5 s apart:
`track 1|1`, `track 3|1b`, `track 2|T1`, `track 0|T1`; no console errors. No new page files.

shortcut: a track without camera sets draws nothing when the track mode is drawn (the game would
index its empty list); fix if such a track ever shows F2. Not proven end to end: which draws a
real session sees (the desktop's frame times are its own).

## Quick proofs on this task's code

`sh tools/proofs/run_all.sh --quick` (48 min wall clock), 0 skipped, 0 different:

| area | cases | identical |
|---|---|---|
| physics | 9 | 9 |
| fresh (with the `cameras` part) | 59 | 59 |
| sound | 4 | 4 |
| render | 14 | 14 |
| gui | 29 | 29 |
| python | 1 | 1 |
| replays | 6 | 6 |
| net | 16 | 16 |
| browser | 9 | 9 |
| ai_page | 9 | 9 |
| shaders | 4 | 4 |
| launcher | 1 | 1 |
| dumps | 6 | 6 |

The `cameras` part of `fresh.sh` in full (3 seeds: start, free, driving, track and random
cameras, plus the collider boxes): 16 of 16 IDENTICAL.

## Not done

- `camera_manager.ini [FADE] COLOR`: done in Task 46b (step 7b).

- Task 46b: F5 onboard free, helicopter, the cinematic replay camera (`video_editing.ini`),
  remembering the last camera, and comparing the camera the replay spinner, camera editor,
  chase-cam editor and photo mode move.
- Task 47: TrackIR, with VR, proven with a stand-in like VR's.
