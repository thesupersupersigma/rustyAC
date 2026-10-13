# The pit crew, AC's replays (Task 28c)

## Resume here

State at v0.28.2:

- **Done and proven**
  - The four tyre changers and their wheel guns (`PitCrew`): `crates/rustyac-render/src/tyre_crew.rs`,
    `crates/rustyac-audio/src/pit_crew.rs`, wired into rustyac.exe.
  - AC's replays: `crates/rustyac-physics/src/replay/` (`ReplayRecorder`, `ReplayManager`: recording,
    the ring, the file of format 16 and 15, the playback's state machine, `getPhysicsState`), wired into
    rustyac.exe (`crates/rustyac-game/src/ac_replay.rs`, `desktop.rs`): recording, autosave, save, the
    replay of a running session, `--replay <file.acreplay>`.
- **Not done**: the ghost car and comparative telemetry (Part 3: only read out, in
  `re/scratch/task28c/ghost.md`), the camera's move out of the car during a stop, the replay's GUI.
  See "Not done".
- **To check the port again** (all read-only on the game; nothing is written outside this repo):
  - `sh re/scratch/task28c/render_crew.sh`: the crew in the game's renderer and the port's (5
    sequences, about 6 minutes). Needs a copy of the render oracle at
    `re/scratch/task28c/bin/render_oracle_crew.exe`.
  - `tools/audio_oracle/target/release/audio_oracle.exe compare --tape oracle/audio/ks_ferrari_f2004/spa_launch.audiotape --camera chase --frames 700 --crew 20:10 --root re/scratch/task28c/audio_root --out re/scratch/task28c/audio_out`:
    the wheel guns' FMOD calls.
  - `sh re/scratch/task28c/replay.sh a`, `... b`, `... c`: 14 whole sessions with the game's replay
    code recording, saving, loading and playing beside the port's (about 25 minutes). Needs a copy of
    the car oracle at `re/scratch/task28c/bin/car_oracle_replay.exe`.
  - `sh re/scratch/task28c/files.sh`: every file of `Documents\Assetto Corsa\replay` loaded by the
    game's loader and the port's, and played (read only; `bin/car_oracle_file.exe`).
  - `sh re/scratch/task28c/render_replay.sh`: frames and sound of a replay's playback in the game's
    renderer / sound code and the port's.
  - `sh re/scratch/task28c/determinism.sh`: rustyac.exe with 0, 1 and 8 workers, and against the
    v0.28.1 exe (`re/scratch/task28c/tgt_v0281/`, built from the tag).
  - `python re/scratch/task28c/tables.py` prints the tables below from the logs.
  - `cargo test --release -p rustyac-physics --test replays`: made-up drives recorded, saved,
    loaded and played, without the game.
- Scratch files: `re/scratch/task28c/`. The four reading briefs there (`rec.md`, `play.md`, `mgr.md`,
  `ghost.md`) say where every field and every float operation of the game's classes is.

## Summary in plain English

**The pit crew.** When you stop in your pit box and asked for new tyres, the screen fades to
black for a quarter of a second, and four mechanics stand at the four wheels. Each walks up,
unscrews the wheel (the wheel gun sounds), carries the old tyre away while a new one comes,
screws it on (the gun again) and steps back. They are the game's own model and animation
(`pitcrewtyre.kn5`, `.ksanim`), in your car's crew colours; the "new tyre" is a copy of your
car's own wheel. Each mechanic is a little slower or faster than the others, by chance, as in
the game. The stop of 28b already said when it begins and ends; the crew listens to that.

![The tyre changers at work (the port's picture of the render oracle: the game's is the same, byte for byte)](replays_crew.png)

**AC's replays.** rustyAC now has the game's replay: while you drive, a compact snapshot of
every car is taken 33 times a second (or what `replay.ini` says) into a ring of fixed size, with
the sun's angle, the leaderboard and the loose objects of the track. In a replay the physics
stands still and every car is rebuilt from two neighbouring snapshots, so the picture, the sound
and the HUD run as they do live. You can play, pause, rewind, fast-forward, go in slow motion,
step a frame, jump a lap, and save. The file is the game's own format (version 16), so:

- a replay saved by rustyAC is, byte for byte, the file the game's own code writes from the same
  drive (proven by running the game's code beside the port);
- rustyAC plays the replays the game recorded: `rustyac.exe --replay <file.acreplay>`. Ten of the
  twelve files in your `Documents\Assetto Corsa\replay` folder load and play exactly as the game's
  own loader plays them; the other two make the game's own loader crash (see "Your AC replays")
  and play in rustyAC without their leaderboard.

rustyAC writes replays into its own `replay` folder (beside rustyac.exe), never into
`Documents\Assetto Corsa`. Its `.ryin` recordings (the inputs of a drive, simulated again) stay
as they are: they are what the proofs run on. A `.acreplay` is a film of the cars; a `.ryin` is
the drive itself.

**What "proven" means here.** The game's own functions (in `acs.exe`, loaded into a test
program) and the port's were given the same inputs, and everything they produced was compared
bit for bit: 80,190 recorded frames of 14 sessions, 42 saved files, 98,504 rebuilt car states
during playback with every transport function, and the pictures and FMOD calls of replay frames.
Nothing differed.

## What is ported

Addresses are of `acs.exe`.

### The pit crew

| Game | Port |
|---|---|
| `PitCrew::PitCrew` 0x1400a0b80, `CrewMember` 0x1400a0aa0, `loadCrewSkin` 0x1400a30c0 | `tyre_crew::PitCrew::new`, `CrewTimeline::read` |
| `PitCrew::pitCrewUpdate` 0x1400a3f20 | `PitCrew::pit_crew_update` |
| `onPitStopBegin` 0x1400a33e0, `CrewMember::randomizeTimes` 0x1400a4390 | `on_pit_stop_begin` (where each man stands, the mirror for the right-hand wheels, three numbers of `rand()` a man) |
| `updateCurrentPosition` 0x1400a4500 | `update_current_position` (the five states, the animation's position, the car's wheel in the man's hands, the new tyre, the guns) |
| `onPitStopFinished` 0x1400a3b50, `reset` 0x1400a4420 | the same names |
| the handlers 0x1400a2330 (replay), 0x1400a2410 (`evPitStopCommit`), 0x1400a27c0 (`evPitStopOver`), the fade's functor 0x1400a27a0 | `on_replay_status_changed`, `on_pit_stop_commit`, `on_pit_stop_over`, `on_fade_in_finished` |
| `SceneGraphCloner::cloneGraph` 0x140217a40 (`cloneNode` 0x140217a80, `getClonedNode` 0x140217b30; `Node::Node(Node*)` 0x14020d9c0, `Mesh::Mesh(Mesh*)` 0x140225ab0, `SkinnedMesh::SkinnedMesh(SkinnedMesh*)` 0x14022bc50) | `Scene::clone_graph` |
| `Node::visit` 0x14020e610 | `Scene::visit` |
| the two `AudioEvent`s of each man (`event:/common/screw`, `unscrew`) | `rustyac_audio::pit_crew::CrewGuns` |

The stop itself (`rustyac_physics::race::PitStop`) now counts its `evPitStopCommit` and
`evPitStopOver` (`commits`, `overs`, `last_start`, `last_changing_tyres`), so that the picture's
thread misses none. In rustyac.exe the crew is in `render/ac.rs` (`tyre_crew`, `CrewLink`): the
commit starts a fade of 0.25 s, at its black frame the crew begins; the guns' calls go to the
sound after the frame (`GameAudio::crew_*`), with the "is it playing" answers of before the
frame, which are the answers the game's code gets.

### The replays

| Game | Port (`rustyac_physics::replay`) |
|---|---|
| `ReplayRecorder::ReplayRecorder` 0x14015c9e0, `setFrameSize` 0x140162420, `clearFrames` 0x14015d4a0 | `ReplayRecorder::new`, `set_frame_size`, `clear_frames` |
| `ReplayRecorder::recordFrame` 0x1401609d0 (the 272-byte `ReplayFrame`, the 4 bytes of additional info, 4 bytes a wing) | `record_frame` |
| `half_float`'s `float2half` / `half2float` as compiled in (tables 0x1404d0f70, 0x1404d1370, 0x1404ce9d0, 0x1404d09d0, 0x1404d0ad0) | `half::float_to_half`, `half_to_float` |
| `mat44f::toEuler` 0x140053340 and its copies | `to_euler` |
| `mat44f::createFromEulerSafe` 0x140155c30 | `create_from_euler_safe` |
| `ReplayRecorder::getPhysicsState` 0x14015d850 | `get_physics_state` |
| `ReplayRecorder::getWingState` 0x14015ff00, `fetchLapFrames` 0x14015d760 | `get_wing_state`, `fetch_lap_frames` |
| `ReplayRecorder::save` 0x1401622a0, `load` 0x140160220, `convertReplayFormat` 0x14015d4b0 | `save`, `load`, `convert_replay_format` |
| `ReplayManager::ReplayManager` 0x140153130 (`replay.ini`: rate, memory budget, autosave) | `ReplayManager::new`, `size_ring` |
| `onStepCompleted` 0x140158b50, `onLapCompleted` 0x140158a00, the snapshot handlers 0x140154da0 / 0x140154d20 | `on_step_completed`, `on_lap_completed`, `on_lap_leaderboard`, `wants_first_leaderboard` |
| `save` 0x140159d10, `load` 0x140157680 | `save_name`, `save_bytes`, `load` |
| `getReplayFileName` 0x140157260, `getAutoFileName` 0x1401568b0, `clearSaved` 0x1401559f0, the autosave closure 0x1401546d0, `shutdown` 0x14015b1c0, `resetReplay` 0x1401597d0 | `get_replay_file_name`, `get_auto_file_name`, `clear_saved`, `autosave`, `on_new_session`, `reset_replay` |
| `startReplayMode` 0x14015b480, `stopReplayMode` 0x14015b800 | `start_replay_mode`, `stop_replay_mode` (the manager's part) |
| `play` 0x1401591f0, `pause` 0x140159070, `stop` 0x14015b6f0, `rewind` 0x140159b40, `fastForward` 0x140156780, `slowMotion` 0x14015b3b0, `nextFrame` 0x140158900, `nextLap` 0x140158930, `previousLap` 0x1401592a0, `photoMode` 0x140159120, `setNormalizedPosition` 0x14015b0b0, `setCutInPerc` 0x14015af30, `setCutOutPerc` 0x14015aff0 | the same names |
| `updatePlayState` 0x14015b9a0, `updateReplayRecorders` 0x14015bc90, `updateReplayAudio` 0x14015bc00 | `update_play_state`, `update_replay_recorders`, `update_replay_audio` |
| `getLeaderboard` 0x140157040, `getCurrentSunAngle` 0x140156d00, `getNormalizedPosition` 0x140157230, `getNormalizedLapChange` 0x1401570d0, `getLaps` 0x140156d70 | the same names |
| `RaceManager::initReplay` 0x14013e820, `Sim::onReplayModeChanged` 0x14019e0c0, `CarAvatar::onStartReplay` 0x1400d9400 | `rustyac_game::ac_replay` and `desktop.rs` (the session of a file; the physics stands still offline; the cars' states come from their recorders) |

The code has no file, window or sound in it (a file is a vector of bytes; what the game tells
its other parts is a list of events), so it builds for the browser as it is: clippy is clean
for `wasm32-unknown-unknown` and `wasm32-wasip1`. `CarPhysicsState` (the game's 0xb70-byte state)
and the matrix blend `lerp` 0x140108600 moved from the renderer and the multiplayer crate into
`rustyac-physics` (`car_physics_state.rs`, `dxmath.rs`) for the same reason.

The game's quirks are kept, because a file must be read as the game reads it:

- The scene's frames (sun, leaderboard index, loose objects) are saved as they lie in memory,
  neither turned to the ring's start nor cut, while the cars' frames are saved oldest first and
  cut. In a file of a ring that has gone round, the scene is out of step with the cars. (The
  game itself plays such a file that way.)
- A replay entered with no "seconds back" starts with its cursor at the frame count, which is
  the first frame again as soon as it moves (0x14015b562).
- `rewind` and `fastForward` by frames clamp the cursor to 0..1; `setNormalizedPosition(1)` is
  the frame count; the lap jumps are 60 frames past / before the lap's first frame.
- An unrecorded slot's additional info is 0, 255, 0, 0 (the struct's constructor).
- A nation code of more than three letters is read as `AC`.

Three things differ from the game on purpose:

- **Where files go**: `replay/` beside rustyac.exe (or `--replay-dir`), never Documents. The
  save refuses a folder that looks like one of the game's.
- **The extension**: the game's own code adds none (its files get theirs elsewhere); rustyAC
  writes `<name>.acreplay`.
- **What the game leaves undefined**: six bytes of a frame that the game never writes (they hold
  whatever its heap held), the three bytes after a lap's flag (its stack) and three bytes a wing
  are 0 in rustyAC's files.

## How each part was proven

1. **The crew's picture.** The render oracle (`tools/render_oracle`) builds the game's own
   `PitCrew` with its constructor on the stand-in car, calls the game's `onPitStopBegin` and
   `pitCrewUpdate` every frame, and compares every frame's D3D11 command log line by line and
   the WARP picture byte by byte with the port's (`--set crew=<seconds>@<frame>`, with
   `pitstate`, `session`, `rstatus`). The game's side has `AudioEvent` stubbed (a picture has no
   sound) and `ACCameraManager::setMode` stubbed (the picture's camera is the frame's own).
2. **The wheel guns.** The audio oracle (`--crew <frame>:<seconds>`) lays a `PitCrew` out by hand
   with the game's own `AudioEvent`s on the oracle's engine and runs the game's
   `onPitStopBegin` and `updateCurrentPosition` on it; the port's side is the same `PitCrew`
   code as the picture's with its sound through `CrewGuns`. The two FMOD call logs are compared
   line by line. Both sides seed `rand()` with 28.
3. **Recording.** `car_oracle multi --replay` runs a whole session in the game's own physics
   (the oracle of Tasks 24 to 28b), and after every step calls the game's
   `ReplayManager::onStepCompleted` with the game's own `CarPhysicsState`s
   (`Car::getPhysicsState`), on recorders made by the game's constructor. The port is given the
   same states. At the end every slot of every car's ring (frame, additional info, wings), the
   recorders' counters and lap caches, the scene's frames and the manager's counters are
   compared. Laps and leaderboard snapshots are made up by the harness from the cars' lap
   counts and go through the game's `onLapCompleted` and into its global table.
   The game's `ReplayManager` is laid out by hand as its constructor leaves it (the
   constructor needs the game's GUI): the `replay.ini` arithmetic of the constructor is ported
   from the listing and tested, not run against the game.
4. **The saved file.** The game's `ReplayManager::save` writes the file; the port's bytes are
   compared with it. Three files a session: as an autosave writes it (no cut), after the replay
   was entered, and cut from 25% to 75% (through the game's `setCutInPerc` / `setCutOutPerc`).
   The game's Documents folder in the oracle is `oracle_no_documents` below the scratch root
   (`SHGetFolderPathW` is replaced before any game code runs); the harness checks, before it
   calls `save`, that `Path::getDocumentPath` says so and that the folder is inside this
   repository. Nothing was written to `Documents\Assetto Corsa`: its replay folder has the same
   12 files as before.
   One preparation: the oracle clears the game's ring memory after the game sized it, so that
   the bytes the game never writes are 0 on both sides. The three bytes after each lap's flag
   (the game's stack) are left out of the comparison.
5. **Loading and playback.** The game's `ReplayManager::load` reads the saved file into a
   second manager (`Sim::loadTrack` and `Sim::applyCustomWeather` do nothing, `Sim::addCar`
   makes a stand-in car with the game's own recorder); the port loads the same bytes. The
   recorders and scene frames are compared. Then a script of the transport's functions (play,
   slow motion 4 and 8, fast forward twice, rewind, pause, frame by frame, positions, lap jumps,
   cuts, stop, the frame steps) runs on both with frame times of 1/60, 1/144 and 1/20 s mixed;
   after every `updatePlayState` the manager's state, every car's whole `CarPhysicsState` (as
   the game's `getPhysicsState` left it in the car), the wings, the car node's flag, the dirt
   level and every loose object's matrix are compared bit for bit. The track is the game's real
   one (its ray cast and its AI line) against the port's. Not compared: a surface's sector and
   collision category, which the port's track does not keep, and what follows the end of a
   surface's name in the game's buffer.
6. **Replay frames in the renderer and the sound.** The game's rebuilt states of a playback are
   written as tapes; the render oracle draws them with the replay's status events on both
   sides, the audio oracle plays them.
7. **Your files.** `car_oracle replay-file <file> --track <t>`: the game's loader and the
   port's on the same file, read in place; then playback as in 5. The cars are stand-ins (no
   car's data is read: wings as the file counts them, a tank of 100 l, no limiter file).
8. **Determinism.** rustyac.exe headless: three new sessions run live with the replay
   recording, recorded as `.ryin`, replayed with 0, 1 and 8 workers; state hashes and the saved
   replay files compared. The state dumps of those and of older recordings against the exe
   built from the tag v0.28.1.

## Results

### 1. The crew

| # | Sequence | Frames, calls | Command log | Pixels |
|---|---|---|---|---|
| 1 | Spa, F2004, from the side: a 10 s tyre change, the first frames (the men walk up) | 39; 216,974 calls | IDENTICAL | IDENTICAL |
| 2 | Spa, F2004, from far: a whole stop of 4 s (walk up, unscrew, change, screw, finish), done, 30 frames after | 429; 2,081,153 calls | IDENTICAL | IDENTICAL |
| 3 | Magione, E30, from the front: a stop, a replay entered (the crew goes, the wheels come back) and left | 189; 1,093,670 calls | IDENTICAL | IDENTICAL |
| 4 | Spa, F2004, from the rear with the mirrors: a stop broken off (reset) | 139; 842,815 calls | IDENTICAL | IDENTICAL |
| 5 | Spa, F2004, on board: a stop, then a session in which the crew does not work | 89; 543,609 calls | IDENTICAL | IDENTICAL |

The wheel guns (700 frames, a tyre change of 10 s from frame 20, Spa, F2004): 275,376 lines of
FMOD calls, the game's run twice and the port's: **identical**. Each of the eight events starts
once and stops once.

In rustyac.exe itself (a practice with an AI driver in the player's car, in a window): the stop
is committed, the fade runs, the crew begins (`Starting Pit animation`, the game's own line) and
the program goes on; no picture was taken there (see "Not done": the camera).

### 2. Replays: 14 sessions

DETERMINISM_TABLES

### 6. Tests and builds

- `cargo test --release --workspace --locked`: 284 passed, 0 failed (4 new: `tests/replays.rs`;
  2 new unit tests of the 16-bit floats).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean; the same for
  `wasm32-unknown-unknown` (`rustyac-web`) and `wasm32-wasip1` (math, ode, content, physics, game).
- The three oracles build (`tools/car_oracle`, `render_oracle`, `audio_oracle`).

## How to watch a replay

**A replay of the session you are driving.** While driving, press the command key and `R`
(the command key is `Right Ctrl` or `Alt`, as for the other commands; `controls.ini`'s
`[START_REPLAY]`). The driving stands still and the replay of the session so far plays from its
start. The same key goes back to the driving.

**A file.** `rustyac.exe --replay <file.acreplay>`: the file names its track and cars; they are
loaded and the replay plays. It may be one of rustyAC's own or one the game recorded, from any
folder, for example:

```
rustyac.exe --replay "%USERPROFILE%\Documents\Assetto Corsa\replay\temp\<name>.acreplay"
```

(the file is only read). A `.ryin` file after `--replay` is rustyAC's own recording, as before.

**Keys in a replay** (the commands of AC's `controls.ini`, with the command key):

| Key | Does |
|---|---|
| command + `Space`, or `P` | play / pause |
| command + `F` | faster forward (each press one step more) |
| command + `D` | backwards (each press one step faster) |
| command + `S` | slow motion (half speed), again: normal speed |
| `.` (full stop) | one frame on |
| command + `N` / `Page Up` | the next lap of the car in view |
| command + `P` / `Page Down` | the lap before |
| `Home` | stop: back to the start |
| `F10` | save |
| command + number pad `3` / `1` / `2`, `F4` | the next car / the car before / your car |
| `F1`, `F6` | the views, the car's own cameras |
| command + `R` | back to the driving (not in a replay loaded from a file) |

A line of text in the middle of the screen says where the replay is and what it does:
`REPLAY 20.2 s / 26.4 s  play  (frame 675 of 879)`.

![A replay in rustyac.exe: the car on the track, the status line](replays_playback.png)

**Where files are saved.** Into `replay\` beside rustyac.exe (`--replay-dir <folder>` for another
place); never into `Documents\Assetto Corsa`.

- `F10` saves `replay\AC_<day><month><year>-<hour><minute><second>_M_<car>_<track>_<layout>.acreplay`.
- With `[AUTOSAVE] ENABLED=1` in `replay.ini`, a new session saves the last one's replay into
  `replay\temp\` under the game's names (`..._R_...` race, `_Q_` qualifying, `_O_` others), keeps
  as many of each kind as the ini says, and starts recording again; so does leaving the program.
  With autosave off, leaving the program writes `replay\cr.acreplay` (the game's "current
  replay").
- `replay.ini` is read from `Documents\Assetto Corsa\cfg` (read only), else from the game's
  `cfg` folder: `[QUALITY] LEVEL` (0 to 4: 8, 11, 17, 33, 67 frames a second), `[REPLAY]
  MAX_SIZE_MB` (10 to 1000; the ring holds that many megabytes for all cars together),
  `[AUTOSAVE]`. Without the file (or with `--no-race-ini`): 33 frames a second, 10 minutes.

Options: `--save-replay <file>` writes the session's replay to a file when the run ends (also
`--headless`, also while a `.ryin` is replayed); `--no-ac-replay` records none; `--replay-script
replay@5,ffwd@6,rewind@8,slow@10,frame@11,pause@12,nextlap@13,prevlap@14,stop@15,save@16,exit@17`
presses the keys by the clock (for checks).

What a replay shows of a car is what the game's replay shows: the body, the suspension and the
wheels, the engine speed, the pedals and the steering, the gear, the lights, DRS and horn, the
damage zones, the tyres' dirt, the fuel, the lap times, the wings. What a frame does not hold is
not there in a replay (tyre temperatures, the forces on the tyres): the game's apps show their
resting values then, and so does rustyAC's HUD.

## Your AC replays

All twelve files of `Documents\Assetto Corsa\replay\temp` were read where they are (nothing was
copied or changed; the folder holds the same 12 files). All are format version 16; none is
version 15. Each has a block of Custom Shaders Patch after the game's own data, which neither
the game's loader nor the port reads.

REAL_FILES_TABLE

Two files make the game's own loader crash. Their leaderboard table at the end holds snapshots
with fewer entries than the file has cars; the game's `save` writes each snapshot as long as it
is, the game's `load` reads as many entries as there are cars, runs out of step, takes the next
bytes for a car number and asks the session for that car (a null pointer). The port sees a car
number outside the file's cars, leaves the leaderboard snapshots out and plays the file (checked
with the port alone: loaded, 1500 frames played). The Custom Shaders Patch in your game plays
them; plain `acs.exe` would not.

One of the Spa files (24 cars, qualifying) was also opened in rustyac.exe itself:
`re/scratch/task28c/exe/real_spa.png` (not in the repository). The loose objects of a file are
only moved when the file counts as many as rustyAC finds on the track; your Spa files count 12
(written with the patch), rustyAC's Spa has 6, so they stay where they are.

## Not done

- **The ghost car and comparative telemetry** (Part 3: `GhostCarRecorder`, `GhostCar`,
  `ComparativeTelemetryRecorder`). Read out in full in `re/scratch/task28c/ghost.md` (layouts,
  the 129 ms sampling, when a lap becomes the ghost, the `.ghost` and `.tc` files byte by byte,
  the playback's frame and blend, the ghost's opacity by distance); nothing of it is ported or
  wired. A follow-up starts from that brief; the matrix blend the ghost uses (`lerp`
  0x140108600) is in `rustyac_physics::dxmath` already.
- **The camera's move out of the car during a pit stop.** The game does it in the crew's code:
  `ACCameraManager::setMode(eOnBoardFree)`, `CameraOnBoardFree::forceThetaPhi(0.1, 0.1, 5)` and
  `lockCamera`, and back to the old view at the stop's end. rustyAC has no free on-board camera
  (`CameraOnBoardFree` 0x140115100 is not ported). The crew's code asks for it through
  `CrewHost` (`set_camera_mode`, `force_theta_phi`, `lock_camera`), rustyac.exe collects the
  requests (`CrewLink::camera`) and drops them: the view stays where it is. To see the crew,
  use the chase view.
- **The replay's GUI** (`ReplayInterface`: the bar with its slider, the cut markers, the save
  box, photo mode): the UI phase. The cuts (`setCutInPerc` / `setCutOutPerc`) and photo mode are
  ported and proven but have no key.
- **The replay's own cameras.** The game switches to the track cameras when a replay starts
  (`ACCameraManager::setMode(eTrack)`); rustyAC has no track cameras yet, the view stays the
  driving view.
- **The body's dirt in rustyAC's own recordings.** The game's recorder reads the car's dirt
  from the picture (`DynamicCarEffects`), at the picture's pace. rustyac.exe records 0 there, so
  that a recording does not depend on the frame rate; a file of the game plays with its recorded
  dirt in the oracle's proof, but rustyac.exe does not yet hand the played dirt to its picture.
  Also not recorded by rustyac.exe: the bodywork's sound level (it is the sound's own value) and
  the driver's button flags other than the lights and DRS.
- **The game's `ReplayManager` constructor against the game.** It needs the GUI; its
  `replay.ini` arithmetic is ported from the listing and tested with the numbers of your files
  (26 MB and 24 cars give 3982 frames, which is what your Spa files hold).
- **Version 15 files against the game.** None exists on this PC. `convertReplayFormat` is
  ported from the listing and tested on a made-up frame only.
- **Online.** The game keeps its physics running during a replay online; rustyac.exe does that
  too (it does not pause), but no live server run was made with a replay.
- **The browser.** The replay code builds for it; the browser preview has no replay keys yet.

## Open questions

1. **`--replay` now takes both kinds of file.** A file that starts with a small number (the
   format's version) is one of AC's replays; anything else is a `.ryin` recording, as before.
   Chosen so that one option does both.
2. **rustyAC adds the extension `.acreplay`.** The game's code adds none; the files in your
   folder have it. Without it rustyAC's files would not be recognised by the launchers.
3. **`F10` saves under the autosave's name with the letter `M`** ("by hand"). The game asks for a
   name in its GUI; there is none yet.
4. **Recording is on in the window, off in `--headless`** unless `--save-replay` asks for it.
   The headless runs are the proofs; they should not write `replay\cr.acreplay` each time.
5. **Autosave follows `replay.ini` as the game does**, including the file `cr` at exit when
   autosave is off. If you would rather have no file at exit, say so: it is one line.
6. **Undefined bytes are written as 0.** The game writes whatever its memory held there. A
   reader cannot tell, and files are the same from run to run this way.
7. **A leaderboard table out of step is left out** instead of crashing as the game's loader
   does (two of your files).
8. **The lap list and leaderboard snapshots in rustyAC's own recordings** come from each car's
   lap counter (a snapshot holds every car's best lap and lap count); the game takes them from
   its `RaceManager::getLeaderboard`. They are in the file in the game's layout; their content
   is plainer than the game's (no total times).
9. **The recorded sun angle** is the session's fixed angle (-16 when none is given); the game
   records its lighting's angle, which moves with the time of day.
10. **In a replay the game's quirk of the first frame is kept** (`getNormalizedPosition` is 1 at
    the very start). The status line shows it as frame 0.
