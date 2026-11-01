# AC's cameras (Task 46)

## Resume here

- Step 0 done: pit_mouse crash fixed; the page's first start compared with a new desktop player;
  606 old kept results (36 GB) moved to the server, SHA-256 matched (below).
- Step 1 done: the audit table below, sent to the user and the split agreed.
- Next: step 2a, the per-frame camera-matrix compare for the views that already run (cockpit, chase,
  chase 2, bonnet, bumper, dash, F6), then the missing ones in the order of the table.

## Step 0: housekeeping

### The GUI pit_mouse crash

The GUI oracle made the escape menu a plain `Control` (0x178 bytes, `gui_ac_hud.rs`); the pit
sequence reads `escMenu+0x1a8` (the menu's setup screen) and puts in its own only when it reads 0.
That word was past the block, so heap garbage: the game followed it into
`SetupScreen::isFixedFuel`. The block is now 0x400 zeroed bytes. GUI `pit_mouse` 10 runs: command
log and pixels IDENTICAL in all 74 frames every time, no crash.

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
| F1 cockpit (`CameraOnBoard`) | F1 | done, not proven | the render oracle gives both sides one fixed camera |
| F1 drivable views (`CameraDrivableManager`), `chase_cam.ini` | F1, C | done, not proven | |
| F6 car cameras (`CameraCarManager`) | F6 | done, not proven | |
| Python `setCameraMode` 1/2/6, `freeCamera*` | apps | done, not proven | |
| G-force head movement, shake (`CameraOnBoard::update`) | | missing | |
| Look left / right / back (`lookLeftRight`, `lookBack`, `updateLook`) | look keys | missing | |
| FOV (`setPlayerFov`), seat position | Numpad +/-, seat keys | missing | |
| Fade-in (`fadeIn`) | | missing | |
| F3 track cameras and sets (`CameraTrack`) | F3 | missing | |
| F2 random camera (`stepCameraRandom`) | F2 | missing | |
| F5 onboard free (`CameraOnBoardFree`) | F5 | missing → 46b | |
| Helicopter (`CameraHelicopter`) | | missing → 46b | |
| Cinematic free camera (`video_editing.ini`) | replay | missing → 46b | |
| Last camera (`loadPreviousSessionUserCamera`) | | missing → 46b | |
| TrackIR | | missing → 47 | |
| Page (Full mode) | F1, C, F4, F6, F7 | as desktop | F2, F3, F5, other cars' views missing |

## Not done

- Task 46b: F5 onboard free, helicopter, the cinematic replay camera (`video_editing.ini`),
  remembering the last camera, and comparing the camera the replay spinner, camera editor,
  chase-cam editor and photo mode move.
- Task 47: TrackIR, with VR, proven with a stand-in like VR's.
