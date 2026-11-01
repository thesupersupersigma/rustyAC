# AC's cameras (Task 46)

## Resume here

- Step 0 done: pit_mouse crash fixed; the page's first start compared with a new desktop player;
  606 old kept results (36 GB) moved to the server, SHA-256 matched (below).
- Step 1 done: the audit table below, sent to the user and the split agreed.
- Step 2a done: the cockpit, the five drivable views and F6 against the game frame by frame
  (`driving_camera.tape`), with the g forces, shake and look keys in the same code; IDENTICAL.
- Next: step 2b, the settings that drive them (`camera_onboard.ini`, `camera_drivable.ini`, the
  car's look-back points) and the look keys, FOV and seat keys in the desktop and the page; then
  fade-in, F3, F2.

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
| G-force head movement, shake (`CameraOnBoard::update`) | | math proven (step 2a) | settings from `camera_onboard.ini` not read yet (zero g, no shake) |
| Look left / right / back (`lookLeftRight`, `lookBack`, `updateLook`, chase look) | look keys | math proven (step 2a) | the keys and the look-back points not wired yet |
| FOV (`setPlayerFov`), seat position | Numpad +/-, seat keys | missing | |
| Fade-in (`fadeIn`) | F4, menus | half | the focus change's fade is ported and proven (`focus.rs`, sequence 18); the menus' `SimAction::FadeIn` (restart) does nothing on desktop |
| F3 track cameras and sets (`CameraTrack`) | F3 | missing | |
| F2 random camera (`stepCameraRandom`) | F2 | missing | |
| F5 onboard free (`CameraOnBoardFree`) | F5 | missing → 46b | |
| Helicopter (`CameraHelicopter`) | | missing → 46b | |
| Cinematic free camera (`video_editing.ini`) | replay | missing → 46b | |
| Last camera (`loadPreviousSessionUserCamera`) | | missing → 46b | |
| TrackIR | | missing → 47 | |
| Page (Full mode) | F1, C, F4, F6, F7 | as desktop | F2, F3, F5, other cars' views missing |

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

## Not done

- Task 46b: F5 onboard free, helicopter, the cinematic replay camera (`video_editing.ini`),
  remembering the last camera, and comparing the camera the replay spinner, camera editor,
  chase-cam editor and photo mode move.
- Task 47: TrackIR, with VR, proven with a stand-in like VR's.
