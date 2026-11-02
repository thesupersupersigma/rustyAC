# Where rustyAC differs from Assetto Corsa on purpose

Everything else runs the game's own rules, checked against acs.exe. These are the places rustyAC
does something else on purpose, each with its reason and the report that has the detail.

## Files

- **The write guard.** Every file rustyAC writes, makes, renames, copies or deletes goes through
  `rustyac_content::guard`, which refuses any path inside the AC install or the player's
  `Documents\Assetto Corsa`. The game writes into both. Why: rustyAC must never change the
  player's game or its settings (a windowed run changes into the AC folder for the Python apps,
  so a relative path would land there). Python apps' own writes are theirs. (leftovers_43b.md)
- **rustyAC's own folder instead of Documents.** Its cfg (camera_manager.ini, gameplay.ini,
  python.ini, acos.ini, …), TempRuns, savedData, setups, replay, GhostCar, ctelemetry, out,
  screenshots and the shadow cache sit beside rustyac.exe; `--cfg-dir` moves all of them (the
  rest beside its folder). Settings are read from this copy first, then Documents'. Why: as above.
  (proofs.md Task 46c, cameras.md, python_apps.md)
- **race_out.json** goes to `out\` (or `--out-dir`), never Documents; same format, copy it over
  for Content Manager. (race_rules.md)
- **FFB Quick Adjust / `saveff`** writes rustyAC's copy `cfg\content\cars\<car>\data\car.ini`, not
  the car folder's. (apps_user.md, apps_telemetry.md)
- **AI lines and track limits**: the game rewrites old `fast_lane.ai` files and saves recomputed
  limits into the track folder; rustyAC computes both in memory on every load. (ai.md, content_tools.md)
- **py_log.txt** goes to rustyAC's logs folder. (python_apps.md)
- **Replays** go to `replay\` (or `--replay-dir`), get the `.acreplay` extension (launchers know
  it), write the format's undefined bytes as 0 (the same run makes the same file), and saving
  refuses a folder that looks like the game's. (replays.md)
- **Ghost car and car telemetry** go to `GhostCar\` and `ctelemetry\player\` beside the exe;
  telemetry is sampled every physics step; dirt is stored as 0 (frame-rate free); nothing is
  recorded in `--headless` runs. (replays.md)

## rustyAC's own files and settings

- **rustyac_controls.ini**: rustyAC's copy of the bindings, made once from AC's controls.ini, with
  rustyAC-only keys (`[RUSTYAC] COMMAND_MODIFIER`, `[RUSTYAC_KEYS_2]`). (conditions.md, awd_ers.md)
- **rustyac_net.ini**: the online id when Steam is not running, made once (`--net-identity`
  names another file). (multiplayer.md)
- **`[KEYBOARD] RUSTYAC_SPEED_SENSITIVITY`**: rustyAC's own; 1 or no key is AC's rule. (web_launcher.md)

## Identity and Steam

- The player id is Steam's when Steam runs (asked as the game asks), else rustyac_net.ini's;
  `--no-steam` forces the latter. race.ini's `GUID` is not read (it could be someone else's). rustyAC
  never prints or stores the Steam id. (multiplayer.md)
- The launcher without Steam: no `SteamAPI.Init` or overlay calls, `isowner` True, no achievements,
  the name from rustyAC's setting; `getguid` / `getid` answer the id rustyac.exe joins with (the
  original makes a new GUID per call without Steam). (launcher.md, launcher_online.md)

## Launcher and page

- Start writes race.ini and the other cfg files into rustyAC's `cfg\` and reads `out\race_out.json`
  back. On first start the cfg files it lacks are copied from Documents' cfg (only read).
  (launcher.md, web_launcher.md)
- The controls page is rustyAC's own (`crates/rustyac-launcher/src/controls.html`). The player name:
  `--player-name`, the launcher's profile.ini, Documents' race.ini, then "You, the player". (web_launcher.md, launcher.md)
- acShowroom.exe is not ported: AC's own works on the same files. (README.md)

## Cameras

- **F2 / F3 on a track without camera sets.** acs.exe crashes (`nextSet` 0x1400cbc90 reads an
  empty list's first set: F3 a second time, or F2's first track camera). rustyAC keeps the camera
  still, as `CameraTrack::update` does without sets, and writes one line to its log
  (`CAMERA: the track has no camera sets …`, once a run), no message on screen. No shipped track
  lacks `cameras.ini`. (cameras.md)

## Crashes and stops the game has that rustyAC avoids

- **No Python DLL**: the game exits ("Failed to load Python, exiting"); rustyAC says so and runs
  without apps. (python_apps.md)
- **Python calls that crash the game** (`getCarMinHeight`, `getCarLeaderboardPosition`,
  `getCarRealTimeLeaderboardPosition`, `getLastSplits`, `getCurrentSplits`, `isCameraOnBoard`,
  `getCameraCarCount` with a bad car, `getCarState` 41/42/55/58 with wheel -1 or 4,
  `printErr` with an exception object, `ac.log(5)`): rustyAC returns -1, clamps or prints an empty
  message; `getPosition` with a bad handle gives -1 (the game: NULLs). (python_apps.md)
- **Replays the game's loader crashes on** (a leaderboard snapshot with fewer entries): the port
  leaves the snapshots out and plays them. (replays.md)
- **Console `testCrash`**: rustyAC prints what the game would do and goes on. (apps_telemetry.md)
- **`[PHYSICS_THREADING] THREADS=1`**: the game throws every step; rustyAC runs one worker. (several_cars.md)
- **A bad `.lut` line**: the game crashes; the port reports the error. (tyre_step.md)
- **Cars refused with a message**: no usable power curve (the game crashes), `[PUSH_TO_PASS]`
  without a turbo (a critical error in the game), an unknown `[TRACTION] TYPE` (the game warns and
  drives it as rear-wheel drive). (drivetrain.md)
- **A missing track `MODEL_n`** is left out with a message; only a track with every model missing
  is refused (the game would let the car fall). (tracks.md)
- **Bad spawn slots**: the game puts a car at the origin or on a taken slot; rustyAC reuses slots
  from the first and says so. A car without a model is left out with a console line. (draw_hear_cars.md)
- **Audio device lost**: AC stops; rustyAC prints a line and plays on silently. (audio.md)

## Repeatability

- The renderer's random numbers are kept apart from the physics' (one stream in the game, so a
  picture option could change the physics): replays stay bit-exact. (renderer_scene.md)
- The AI drives as AC does without the physics thread pool, at any thread count (AC with the pool
  is not repeatable). (ai.md)
- Online: the pong carries the physics clock, not the wall clock (sessions replay from tapes).
  (multiplayer.md)

## Online

- Non-ASCII names are read as Latin-1 (the game shows a wrong character); a server's NaN numbers
  are dropped. (multiplayer.md)
- Keyboard drivers keep the quick pit menu and the strategy page (the game gives them only the
  mouse pit menu). (online_menus.md)
