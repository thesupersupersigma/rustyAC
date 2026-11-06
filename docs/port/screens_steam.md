# Triple screens, TrackIR, Steam and VR (Tasks 47, 47b, 47c and 47d)

## Task 47d: Steam achievements, the forced virtual mirror, the page's app files

### Resume here

Task 47d is done (v0.47.3). Nothing is left in flight; leftovers are under "Not done" and go to
Task 47e. Scratch: `re/scratch/task47d` (the machine-code briefs `brief_achievements.md`,
`brief_leaderboards.md`, `brief_triggers.md`; step 3's runs under `step3/`).

How to turn achievements on: `rustyac.exe --steam-achievements on`, or `[STEAM] ACHIEVEMENTS=1`
in rustyAC's own `cfg\rustyac.ini`. Off by default. It also needs Steam running (the game's
`steam_api64.dll` loaded and `SteamAPI_Init` working); with it off nothing of `SteamInterface`
is made and none of its Steam calls happen.

### End proofs

`sh tools/proofs/run_all.sh --quick`: every area identical. The first run was cut short by the
PC running out of memory; render, gui, fresh, dumps, net and ai_page were run again on their own
(`CARGO_BUILD_JOBS=4`), the rest are from the first run.

| area | cases | identical | different | skipped |
|---|---|---|---|---|
| physics | 9 | 9 | 0 | 0 |
| fresh | 62 | 62 | 0 | 0 |
| sound | 4 | 4 | 0 | 0 |
| render | 14 | 14 | 0 | 0 |
| gui | 29 | 29 | 0 | 0 |
| python | 1 | 1 | 0 | 0 |
| replays | 6 | 6 | 0 | 0 |
| net | 16 | 16 | 0 | 0 |
| browser | 9 | 9 | 0 | 0 |
| ai_page | 9 | 9 | 0 | 0 |
| shaders | 4 | 4 | 0 | 0 |
| launcher | 1 | 1 | 0 | 0 |
| dumps | 6 | 6 | 0 | 0 |
| all | 170 | 170 | 0 | 0 |

### Step 1: achievements, stats and leaderboards

`SteamInterface` (constructor 0x1401a5d70, `update` 0x1401accd0, destructor 0x1401a6ec0) is
ported in `crates/rustyac-game/src/achievements.rs`, every Steam call through the flat API and
the interfaces' vtables in the game's order, so the stand-in DLL sees the same calls. What it
sends, as the game does:

- Achievements (`SetAchievement` + `StoreStats`, a system message per unlock): 100/200/300 km/h,
  a clean drive, a crash, hotlapper, time attack 3/5/10, practice 10 laps, practice with no
  aids, 50/100/250 km driven, drift combos 10/25/50 and drift levels, the four car-and-track
  lap times (ferrari_458 imola, lotus_49, tatuusfa1, bmw_m3_gt2; standard and `_pro`), an
  official event's tiers.
- Stats, each +1 when the drive starts: `runs`, `games`, the car (without a `_step1`, `_drift`,
  `_S1`... suffix), the track, `drift_mode` / `time_attack_mode`, or `replay_mode` instead in a
  replay, and `controls_x360` / `controls_keyboard` / `controls_wheel` from `controls.ini
  [HEADER] INPUT_METHOD`.
- Leaderboards: the session's board `_lb_HOTLAP_<car>_<track>` (likewise `_lb_TIME_ATTACK_`,
  `_lb_DRIFT_`) is found, a valid clean lap's time or the drift run's points uploaded, and the
  friends' rows downloaded for the Friends Leaderboard; the player's avatar is fetched as a
  184×184 texture (`fetchPlayerImage`).
- None of it in a replay (only its stat), or with penalties off (no checks, no board).

The proof: `car_oracle achievements-micro --seed n` runs the game's own `SteamInterface` on nine
scripted drives against the stand-in `steam_api64.dll` (`tools/car_oracle/src/game/steam_standin.rs`,
every call and argument written down, made-up answers, failures included), and
`crates/rustyac-game/examples/achievements_check.rs` runs the port on the same inputs and
answers. Cases: practice (speeds, distance, both times to beat, ten laps without aids),
penalties off, a mod's hotlap event (the board, its rows, scores), an official hotlap with its
tiers and a failed upload, time attack with no board found, drift (combos, levels, a score), a
race crash, drag with Steam failing everywhere, a replay. In `tools/proofs/fresh.sh`.

| Seed | Lines | Cases | Result |
|---|---|---|---|
| 1 | 43,491 | 9 | IDENTICAL |
| 2 | 43,459 | 9 | IDENTICAL |
| 3 | 43,536 | 9 | IDENTICAL |

In rustyac.exe the port runs on the physics thread (`Live`, once a drawn frame), and its
unlock messages show in the GUI. A headless run without `--drawn-frames` has no drawn frames,
so it makes no achievement checks.

### Step 2: the server's forced virtual mirror

The handshake's FORCE_VIRTUAL_MIRROR byte is Sim+0x1e3 (`isVirtualMirrorForced`). Ghidra's
pseudo-C calls it `useMousePitstop` (that field is Sim+0x1e4) in `ACClient::handshakeTCP` and in
`VirtualMirrorRenderer`'s constructor, `update` and `shutdown`; the machine code reads 0x1e3.
Now, online on a forced server with no headset, the mirror starts on and F11 keeps it on with
the game's message "Virtual Mirror is forced ON by the Server" (`rustyac_render::mirror::f11`,
`VirtualMirrorRenderer::update` 0x1401d2190); with a headset F11 does nothing, as in the game.

The proof: `car_oracle mirror-micro --seed n` runs the game's `VirtualMirrorRenderer`
(constructor 0x1401d1ab0, `update`) with a stand-in `GetAsyncKeyState`, eight cases a seed
(headset, forced, starting state, on and off), every frame's state and system message written
down; `crates/rustyac-game/examples/mirror_check.rs` runs the port. Seeds 1, 2, 3: IDENTICAL. In
`tools/proofs/fresh.sh`.

### Step 3: the page saves the apps' files when a drive ends

The game writes the apps' files at shutdown (`GameScreen::shutdown`); the page has no shutdown,
so it now does the same when a drive ends. `GameApps::kept` keeps the files `write_files` would
write (same paths, same bytes); `full::shutdown_files` shuts the apps down and returns them;
`Game::shutdown_files` gives them to `stopDriving` in `web/app.js`, which saves each in the
page's Documents (rustyAC's `rustyac/cfg/x.ini` is the page's Documents `cfg/x.ini`, likewise
`savedData/`). At the next drive the page also hands its saved `cfg/triple_screen.ini` to the
game (before only `video.ini` went in), so a triple drive starts from the last save.

Proofs, on a triple-screen drive at 1920×1080 on magione in the bmw_m3_e30 that clicks the
Triple screen app's spinners (the same clicks both sides):

- `tools/web_checks/task47d/shutdown_files.sh`: rustyac.exe against the page's code natively
  (`full_frame --shutdown`): 3 files, 3 IDENTICAL (`cfg/telemetry_user.ini` 28 bytes,
  `cfg/triple_screen.ini` 325 bytes, changed by the clicks,
  `savedData/driverPerformances/magione/bmw_m3_e30.ini` 933 bytes).
- `tools/web_checks/task47d/triple_live.sh`: the page live in Chrome (Full mode, `?guido=` the
  same clicks, the drive ended by AC's pause menu Exit), the files read back from the page's
  saved Documents against the desktop's: 3 files, 3 IDENTICAL.

The post-processing filter, the chat export and force feedback's car.ini take the same
`write_files` path; this session makes none of them, so they are not in the proof.
`GameGui::shutdown`'s own files (acos.ini, gameplay.ini, exposure.ini, the setup's last.ini) are
not apps' files and the page still does not save them.

### Not done (Task 47e)

- The Friends Leaderboard rows and the avatar are made and kept but not drawn
  (`FriendsLeaderboardDisplayer`).
- A headless run without `--drawn-frames` makes no achievement checks (they run once a drawn
  frame, as in the game).
- `MOTION_TRACKER=RIGHT_HAND` with no right controller (a proof case comparing what is drawn,
  nothing, with the game's NaN signs): not started.
- Post-processing in VR (Task 47c).
- The page does not save `GameGui::shutdown`'s acos.ini, gameplay.ini, exposure.ini and last.ini.
- `car_oracle`'s clippy with `-D warnings` fails on about 70 older lints rustc 1.97 now reports
  (none in this task's files).
- Nothing was tried on a real Steam account (the rule): only the stand-in DLL.

## Task 47c: triple screens and VR, the rest

### Resume here

Task 47c is done (v0.47.2). Nothing is left in flight; leftovers are under "Not done" and go to
Task 47e (47d is Steam achievements). Notes per step: `re/scratch/task47c/step1.md` .. `step6.md`,
the Triple screen app's machine-code spec `tsm.md`.

How to run rustyac.exe in VR without a headset: `re/scratch/task47c/vrroot.ps1` makes an AC root
of junctions whose `system/x64/openvr_api.dll` is the stand-in runtime (`tools/render_oracle`'s
`[lib]`, `cargo build --release --lib` there; remove the root only with `vrroot_rm.ps1`), then
`AC_ROOT=<that root> OPENVR_STANDIN_OUT=<dir> rustyac.exe --video-ini <a video.ini with
[CAMERA] MODE=OPENVR> ...`: the stand-in writes its calls (`calls.txt`) and the first
`OPENVR_STANDIN_FRAMES` (8) frames' eyes (`eye_<frame>_<eye>.png`).

### End proofs

`sh tools/proofs/run_all.sh --quick` (0h53m): 170 cases, 170 identical, 0 different, 0 skipped,
0 vanished.

| area | cases | identical | different | skipped |
|---|---|---|---|---|
| physics | 9 | 9 | 0 | 0 |
| fresh | 62 | 62 | 0 | 0 |
| sound | 4 | 4 | 0 | 0 |
| render | 14 | 14 | 0 | 0 |
| gui | 29 | 29 | 0 | 0 |
| python | 1 | 1 | 0 | 0 |
| replays | 6 | 6 | 0 | 0 |
| net | 16 | 16 | 0 | 0 |
| browser | 9 | 9 | 0 | 0 |
| ai_page | 9 | 9 | 0 | 0 |
| shaders | 4 | 4 | 0 | 0 |
| launcher | 1 | 1 | 0 | 0 |
| dumps | 6 | 6 | 0 | 0 |

The areas this task changed, run on the final code:

| proof | why | result |
|---|---|---|
| render `r47_01..03` (triple, 5760×1080), `r47b_01..08`, `r47c_01..05`, and the overlays cases render.sh adds | the VR cameras, motion trackers, room scale, the triple camera | 31 cases, 31 identical |
| GUI `hud_flags`, `hud_flags_triple`, `hud_flags_vr`, `hud_cars`, `hud_cars%triple`, `hud_messages%triple`, `lb_control_12%triple`, `end_race_12%triple`, `app_openvr`, `app_riftaudio`, `app_triple`, `app_ppfilter` | the VR HUD, name tags, the triple switch, the three new apps | 12 cases, 12 identical |

Clippy clean natively (`--workspace --all-targets --locked`) and on both wasm targets as CI runs
them (`rustyac-web` also `--all-targets` on `wasm32-unknown-unknown`). Free space on C: 56 GB at
the start, 36 GB at the end (the PC's own use between sessions accounts for most of it; this
task's scratch folder is 0.8 GB). No web zip files are new or renamed; the preview pack needs no
new files (the Triple screen app's head picture is under `content/gui`, which the page already
takes).

### Step 1: the headset GUI on the game's frame

`Sim::renderScene` 0x14019e570 starts with `GUI::preRender3D`, `Game::renderHUDOnDemand` and
`GUI::renderVPointer`, before the scene. rustyac.exe drew the GUI after the eyes, so the headset
showed last frame's GUI. Now `AcRenderer::draw` keeps the GUI pass in one closure and runs it
before the scene in VR, in its old place when flat (the flat order, and so its pictures, are
unchanged; the quick run's render and GUI cases show it).

Proof: rustyac.exe with the stand-in runtime and `--gpu-log` (frame 2): the GUI target is bound,
cleared and drawn (log line 49) before the mirror (407), the window (965), the shadow maps (994)
and the two eyes (3482, 6696), the order of the game's side of `r47b_07`. The entry "VR: the GUI
in the headset is a frame late in rustyac.exe" is gone from `differences.md`.

### Step 2: what the game hides in VR

Every reader of `Sim::isVrConnected` (+0x1e1) in the game, and what rustyAC now does with a
headset (`GuiSetup.vr`, `HudInput.vr_connected`, `gui.sim.vr_connected`, the crew's
`is_vr_connected`):

| the game | in VR | rustyAC |
|---|---|---|
| `ACCameraManager`'s fade lambda 0x140032770 | no fade quad (its time still runs) | camera-manager fades and the menu fade not drawn |
| `Sim::Sim` (blur quality) | motion blur quality not doubled | the same |
| `Sim::Sim` (`ProximityIndicator`) | not made | not drawn |
| `Sim::Sim` (`DriverNameDisplayer::setActive(false)`) | name tags off (the key still toggles them) | the same |
| `Sim::Sim`, `VirtualMirrorRenderer` ctor / update | virtual mirror off, F11 does nothing | the same (`--virtual-mirror` too) |
| `DriveThroughPenaltyAvatar` ctor / `renderHUD` | a VR banner: the penalty flag (`FLAG_DT`) at the centre, two centred lines above it | ported (`hud_flags.rs`) |
| `EndSessionDisplayer::displayResults` | results shown, the camera switch skipped | the same (the gate existed, the flag is now set) |
| `PitCrew::onPitStopBegin` | no switch to the pit-stop camera | the same |

The "mouse pit menu" of Task 47b's list is not hidden by the game: the gate it meant is the
server's forced virtual mirror (step 4's finding), and `PitStop::renderHUD` has no VR read.
Proof: GUI sequence `hud_flags_vr` (75 frames) IDENTICAL; `hud_flags`, `hud_flags_triple`,
`hud_cars` still IDENTICAL.

### Step 3: camera keys in VR

No camera key in the game reads `isVrConnected` or a VR class: F1, F2, F3, F5, F6, F7 and the
rest work as when flat, and the stereo camera puts the head's pose on whatever camera is chosen
(its field of view is the headset's). The only VR rules are the first update forcing the cockpit
(Task 47b), the virtual mirror's F11 (step 2) and the cockpit's look and shake (Task 47b).
rustyAC's keys have no VR read either. Proof: new render cases `r47c_01` (free camera at Spa
through OpenVR) and `r47c_02` (side camera at Magione through Oculus) IDENTICAL, every runtime
call and matrix the same; rustyac.exe with the stand-in and `--focus-script track@2,onboard@4.5`
shows the F3 and F5 views in the eyes.

### Step 4: room scale

Nothing in acs.exe 1.16 turns room scale on. The PDB's `Sim` layout is 0x1e0 `useProView`,
0x1e1 `isVrConnected`, 0x1e2 `isRoomVR`, 0x1e3 `isVirtualMirrorForced`, 0x1e4
`useMousePitstop` (Ghidra's decompiler names are one byte off from 0x1e2 on). A byte scan of
the machine code finds no store to +0x1e2 but `Sim::Sim`'s zeroing; it is read only by
`Sim::isVrRoom` and `Sim::createCamera` (the `StereoCameraVive` constructor's last flag). The
room mode (standing tracking space, `[VR_ROOM] ROTATION`) is dead code unless something patches
the game's memory. Proof of the rotation Task 47b wrote: `render_oracle --vr-room` forces the
flag on both sides; `r47c_03` (Spa, the F2004's dash, `ROTATION=2`) IDENTICAL, `SetTrackingSpace(1)`
in both runtime logs.

### Step 5: FormOpenVR, motion trackers, the Rift's sound device

- **FormOpenVR** ("OpenVR", made only with the HTC Vive camera): SteamVR's supersampling with its
  Save button (into rustyAC's own `cfg/openvr.ini`, and SteamVR's `renderTargetMultiplier`) and
  the pupillary distance (live). `app_open_vr.rs`; the oracle's `app_openvr` (101 frames)
  IDENTICAL with its camera calls and openvr.ini.
- **openvr.ini `MOTION_TRACKER`** (`LEFT_HAND`, `RIGHT_HAND`, `TRACKER`): the head seen from a hand
  controller or a tracker, as `UpdateHMDMatrixPose` does. Render cases `r47c_04` (left hand) and
  `r47c_05` (tracker) IDENTICAL. A right hand without a controller reads 64 bytes before the
  pose table in the game (index -1) and draws nothing; the port does the same read and also draws
  nothing, but the NaN signs of the view differ, so it has no case.
- **oculus.ini `AUTOSELECT_RIFT_AUDIO_DEVICE`**: `OculusManager` asks the runtime for the Rift's
  waveOut id and hands it to FMOD as its driver (`rift_audio_driver`, then
  `GameAudio::set_driver`), with the game's messages. GUI sequence `app_riftaudio` (on, none
  found, no runtime functions, off, no key) IDENTICAL with the runtime calls and oculus.ini.

### Step 6: triple screens

- `SessionLeaderboard`, `EndSessionDisplayer` and `SystemNotification` ask
  `Sim::isInTripleScreenMode` but never use the answer (both branches compiled to one): no triple
  path. Proven with the new `%triple` sequence suffix (triple mode from before anything is made):
  `hud_messages%triple`, `lb_control_12%triple`, `end_race_12%triple` IDENTICAL.
  `DriveThroughPenaltyAvatar` and `FuelLight` were ported in Task 29 (`hud_flags_triple`).
- **Name tags** through the three screens (`DriverNameDisplayer::projectPoint`'s triple path: the
  first screen that sees the car, drawn only in the cockpit, drivable and free cameras);
  rustyac.exe no longer hides them in triple mode. `hud_cars%triple` (59 frames) IDENTICAL.
- **The Triple screen app** (`TripleScreenManager`, `app_triple.rs`): the angle, screen width,
  distance and bezel spinners move the camera's screens at once and, when the game closes after a
  change, write `triple_screen.ini` into rustyAC's own cfg folder (the page: its own Documents,
  saved when the drive ends). The camera now reads rustyAC's own triple_screen.ini first, then
  Documents'. GUI sequence `app_triple` (73 frames): commands, pixels, state, the camera's screens
  (bit for bit) and triple_screen.ini (333 bytes) IDENTICAL. rustyac.exe in `MODE=TRIPLE` with an
  unchanged app leaves the file alone, as the game does.

### Post-processing in VR

It waits for the YEBIS tasks. It plugs in where the game's `StereoCameraVive::render` 0x140223720
calls `StereoCameraVive::renderYebis` 0x140223f60 and `StereoCameraForward::render` 0x1402135c0
calls `StereoCameraForward::renderYebis` 0x140213b10 (each eye through YEBIS before it is
submitted): `vr.rs` / `oculus.rs` `render`, which today refuse a video.ini with post-processing
on (their constructors return "VR with post-processing is not done").

### What still needs real hardware

Nothing here was tried on a headset, a real SteamVR or Oculus runtime, trackers, a Rift's sound
device or three monitors: the proofs use the stand-in runtimes and the game's own code beside the
port. The Oculus path in rustyac.exe needs a signed LibOVR, so it was never run there.

### Not done (Task 47e)

- The server's forced virtual mirror online (`FORCE_VIRTUAL_MIRROR`, Sim+0x1e3: mirror on and F11
  locked when flat): rustyAC reads the byte but never applies it.
- The page never runs the other apps' shutdown, so their files (force feedback, telemetry preset,
  chat export and the rest) are not saved in a page drive; only the Triple screen app's file is.
  The page's triple save was not run live in a browser.
- A right hand without a controller (`MOTION_TRACKER=RIGHT_HAND`): NaN signs differ (no case).
- Post-processing in VR (above).

## Task 47b: VR

### Resume here

Task 47b is done (v0.47.1). Nothing is left in flight; leftovers are listed under "Not done" and go
to Task 47d.

- Step 0: no proof case vanished (Task 47's 156 was a miscount of 169); the quick `rays` key now
  matches its case; `run_all.sh` prints cases vanished since the last run of the same mode.
- Step 1: the audit table below.
- Step 2: `rustyac-render` `vr.rs` is the game's `StereoCameraVive` (OpenVR), proven on a stand-in
  runtime (render cases `r47b_01..03`); `rustyac.exe` uses it with video.ini `MODE=OPENVR`.
- Step 3: `oculus.rs` is the game's `StereoCameraForward` (Oculus), proven on a stand-in runtime
  (`r47b_04..06`); `rustyac.exe` uses it with `MODE=OCULUS` and a signed LibOVR.
- Step 4: `gui3d.rs` is the game's GUI in the headset, proven with both runtimes (`r47b_07..08`).
- Step 5: the page has no VR (note below).
- Not done (Task 47d), besides each step's own list: the rest of what VR switches off in the
  game (proximity indicator, driver names, fade quad, penalty banner, end-session results, mouse
  pit menu, pit-stop camera switch): `rustyac.exe` never sets `SimLink.vr_connected` or the crew's
  `is_vr_connected`, so those still show in VR; vsync, the virtual mirror and the flat GUI are off.
- Nothing was tried on a real headset or a real runtime: this PC has neither SteamVR nor the
  Oculus software.

### End proofs

`sh tools/proofs/run_all.sh --quick` (1h09m): 170 cases, 170 identical, 0 different, 0 skipped,
0 vanished (fresh 62: one more than Task 47's 61, the `rays magione -` case the fixed key now
picks).

| area | cases | identical | different | skipped |
|---|---|---|---|---|
| physics | 9 | 9 | 0 | 0 |
| fresh | 62 | 62 | 0 | 0 |
| sound | 4 | 4 | 0 | 0 |
| render | 14 | 14 | 0 | 0 |
| gui | 29 | 29 | 0 | 0 |
| python | 1 | 1 | 0 | 0 |
| replays | 6 | 6 | 0 | 0 |
| net | 16 | 16 | 0 | 0 |
| browser | 9 | 9 | 0 | 0 |
| ai_page | 9 | 9 | 0 | 0 |
| shaders | 4 | 4 | 0 | 0 |
| launcher | 1 | 1 | 0 | 0 |
| dumps | 6 | 6 | 0 | 0 |

The area this task changed, render, also ran its new cases `r47b_01..08` (all 8 identical, with
every runtime call and matrix the same) and the flat HUD cases `r22c_09`, `r22c_13`, `r22c_17`
(identical: the oracle's HUD pass moved into a block the VR frame shares). Clippy clean natively
(`--workspace --all-targets`) and on both wasm targets as CI runs them. Free space on C: 61 GB at
the start, 56 GB at the end. No web zip files are new or renamed.

### Step 0: the case count

The prompt's question: Tasks 46b and 46c's quick runs had 168 cases, Task 47's 156. Nothing
vanished; two reports had the count wrong:

| run | cases in its log | the report said |
|---|---|---|
| Task 46 (v0.46.0) | 167 (fresh 59: the cameras part had no orbit camera yet) | 167 |
| Task 46b | 168 (fresh 60: + `cameras 1 orbit cameras`) | 168 |
| Task 46c (twice: `re/scratch/task46c/quick1.txt`, `quick2.txt`) | 168 | 168 |
| Task 47 (`re/scratch/task47/quick_table.md`) | 169 (fresh 61: + `cameras 1 trackir start`) | **156** (a miscount, now 169 below) |

One case never ran in a quick run at all: the quick key `rays magione ?` matched `rays magione`
but `track_survey --list` names the layout `-`, so the case is `rays magione -`. The key is now
`rays magione -?`; quick runs have one more case (fresh 62).

The new check: `run_all.sh` keeps each area's case names of the last run of the same mode in
`target/proofs/cases/<mode>/<area>.txt` and prints in red, with the summary, every case that was
there last time and is not now (also in `target/proofs/vanished.txt`). Cases retired on purpose
go in `tools/proofs/retired.txt` (`<area> <case> | why`) and are not printed. The exit code does
not change. A wiped `target/proofs` starts the lists again.

### Step 1: the audit

Read from `re/decomp` and the disassembly (details, addresses and field offsets in
`re/scratch/task47b/openvr.md`, `oculus.md`, `startup.md`). Port status at the start of 47b:

| feature | the game | port at v0.47.0 |
|---|---|---|
| Start-up, runtime choice | `Sim::createCamera` 0x1401982e0 reads video.ini `[CAMERA] MODE`: `OCULUS` → `StereoCameraForward::initOculus`, `OPENVR` → `StereoCameraVive::initOpenVR`; a runtime that fails to start falls back to the flat camera, silently | reads `TRIPLE` only; OCULUS / OPENVR run flat |
| OpenVR runtime | `openvr_api.dll` delay-loaded; `VR_InitInternal(&err, 1)`, `VR_IsInterfaceVersionValid("IVRSystem_015")`, `VR_GetGenericInterface` for IVRSystem_015, IVRCompositor_020, IVRSettings_002; `VR_GetInitToken` before each use | none |
| Oculus runtime | its own LibOVR shim loads `LibOVRRT64_1.dll` only if Authenticode-signed by "Oculus VR, LLC" (`OVR_Win32_SignCheck` 0x1401dd320); 18 `ovr_*` calls; no headset name → fails | none |
| Per-eye cameras and projections | OpenVR: `GetEyeToHeadTransform` and `GetProjectionMatrix` (near 0.5, far 5000) per eye, composed with the HMD pose in `computeCameraMatrix` 0x140222850; Oculus: `ovr_GetRenderDesc`, `ovr_GetEyePoses`, `ovrMatrix4f_Projection` (near 0.1, far 5000) | none |
| Render targets | OpenVR: `GetRecommendedRenderTargetSize` × `steamvr/renderTargetMultiplier`, a multisampled temp target per eye resolved into two eye textures, `Submit` per eye after `kglSwapBuffers`; Oculus: one texture swap chain per eye, `ovr_SubmitFrame` with one `ovrLayerEyeFov` | none |
| Mirror window | OpenVR: the window cleared to (0.5, 0.1, 0.1) and, if `openvr.ini [MIRROR_TEXTURE] ENABLED`, last frame's left eye drawn over it; Oculus: the left eye's swap-chain texture drawn to the window | none |
| Head pose into the cockpit camera | `WaitGetPoses` / `ovr_GetEyePoses` every frame; HMD pose × eye pose × car cockpit frame; g-force, shake and glance off in `CameraOnBoard::update` unless `[VR] ENABLE_CAMERA_SHAKE` | none |
| HUD and apps | `isRenderingGui` = 0 (no flat GUI pass, no flat swap); the whole desktop and HUD drawn into a window-sized target (`GUI::preRender3D`, `Game::renderHUDOnDemand`) and shown on a 140°, 24-quad cylinder 0.4 m around the driver's eyes, 0.45 m high (`GUI::render3D` 0x140254900), with a mouse pointer (`oculus_pointer.png`) | hook only (`Gui::render_target_3d`) |
| Recentring | first manager update: cockpit camera + recentre; then glance left and right held together → `resetHeadPosition` (OpenVR `ResetSeatedZeroPose`, Oculus `ovr_RecenterTrackingOrigin`); OpenVR also Ctrl+Space | none |
| VR-only settings and files | `openvr.ini` (`[SETTINGS] PUPILLARY_DISTANCE`, `[MIRROR_TEXTURE] ENABLED`, `MOTION_TRACKER`), `oculus.ini`, `[VR] ENABLE_CAMERA_SHAKE`; the in-game OpenVR form (supersampling 0.8–2.4, IPD; writes openvr.ini, `steamvr/ipdOffset` and the render multiplier) | none |
| What VR switches off (`isVrConnected` Sim+0x1e1) | flat GUI (`isRenderingGui`, the Task 45c flag), vsync, proximity indicator, driver names, virtual mirror, fade quad, penalty banner, end-session results, mouse pit menu, pit-stop camera switch, motion-blur quality ×2 | flag exists, always false; pit crew and end session already gate on it |

### Step 2: OpenVR

`crates/rustyac-render/src/vr.rs` is the game's `StereoCameraVive` (constructor 0x140220b80,
`initOpenVR`, `configureOpenVR`, `render`, `renderNoPP`, `normalRendering`,
`renderMirrorTexture`, `finishRendering`, `resetHeadPosition`, `shutdown`), sitting on the
`CameraForward` it draws with (`CameraForward::vr`). It reaches the runtime as the game does:
the six flat functions of `openvr_api.dll` and three interfaces by vtable slot (`OpenVrApi`).

The proof: `render_oracle --vr` (video.ini `[CAMERA] MODE=OPENVR`, Documents cfg/openvr.ini
with the mirror on and `PUPILLARY_DISTANCE=66`) runs the game's own camera with the stand-in
runtime `tools/render_oracle/src/openvr_fake.rs` bound into the game's delay-load table of
`openvr_api.dll` (the shared loader now binds delay-loaded DLLs a tool brings stand-ins for),
and the port's camera with the same functions. The stand-in turns and moves the head every
frame, has canted eyes and an asymmetric projection per eye, holds Ctrl+Space in the third
frame (recentring) and reads back what `Submit` gets. Compared: every runtime call with its
arguments in order (`VR:` line), the head, eye and projection matrices of every frame as bits,
the command log, and one picture holding the window (the left eye of the frame before) over
both eyes.

| case | what | result |
|---|---|---|
| `r47b_01` | Spa, still, 1 frame, 1280x720 window, 640x720 eyes | 64 calls and matrices, log and pixels IDENTICAL |
| `r47b_02` | Spa, the F2004's dash on its lap tape, mirror, 29 frames | 570, IDENTICAL in all 29 frames |
| `r47b_03` | Magione, chase, 4x MSAA (the eyes resolved, not copied), 7 frames | 174, IDENTICAL in all 7 frames |

In `rustyac.exe`: video.ini `[CAMERA] MODE=OPENVR` loads the game's own
`system/x64/openvr_api.dll` and makes the VR camera when a runtime starts (else the plain
camera, with a note, as the game falls back). As `ViveManager` does, the first frame shows the
cockpit and recentres the head, and look left plus look right held together recentre it again
(Ctrl+Space too, inside the camera). The window shows the left eye of the frame before and does
not wait for the display; the flat GUI and HUD are not drawn (`isRenderingGui` is off: the HUD
in the headset is step 4), nor the virtual mirror. The cockpit camera keeps the head at the
eyes (no g forces, no shake) unless the game's system/cfg/assetto_corsa.ini `[VR]
ENABLE_CAMERA_SHAKE` is set, and has no glance (`CameraOnBoard::update`). Not tried live: this
PC has no SteamVR and no headset.

Not done here: post-processing in VR (`renderYebis`; the port refuses `MODE=OPENVR` with
post-processing on), openvr.ini `MOTION_TRACKER`, room scale (`SetTrackingSpace(1)` and
video.ini `[VR_ROOM] ROTATION`: ported, not proven, the game only asks for them with the
virtual mirror forced).

### Step 3: Oculus

`crates/rustyac-render/src/oculus.rs` is the game's `StereoCameraForward` (constructor
0x140211370, `initOculus`, `OculusTexture::Init`, `render`, `renderNoPP`, `oculusRenderPass`,
`normalRendering`, `renderMirrorTexture`, `computeEyePoses`, `finishRendering`,
`resetHeadPosition`) on `CameraForward::oculus`, with LibOVR's own `ovr_CalcEyePoses` and
`ovrMatrix4f_Projection`. It reaches the runtime through the 17 functions the game's LibOVR shim
uses (`OvrApi`). The loader keeps the shim's check: `LibOVRRT64_1.dll` (from `LIBOVR_DLL_DIR`
or the normal search) is used only when Windows trusts its Authenticode signature and the
signer chain is Oculus VR, LLC / DigiCert SHA2 Assured ID Code Signing CA / DigiCert Assured ID
Root CA (`OVR_Win32_SignCheck`, `ValidateCertificateContents`), the file held open until loaded.

The proof: `render_oracle --oculus` (video.ini `MODE=OCULUS`, Documents cfg/oculus.ini as the
game's settings write it) fills the game's shim pointers with the stand-in runtime
`tools/render_oracle/src/oculus_fake.rs` and marks the shim's library loaded, so the game's
signature check does not run there (the stand-in is not signed). The stand-in hands out swap
chains of three textures, an asymmetric field of view per eye, eyes 32 mm to each side, a head
turning and moving every frame, holds Ctrl+Space in one frame and asks for a recentring
(`ShouldRecenter`) in another, and reads back the committed textures each `ovr_SubmitFrame`
names. Compared as for OpenVR: every runtime call with its arguments in order (the submitted
layer with its poses and sample time as bits), the projection and view matrices of every frame,
the command log, the window over both eyes.

| case | what | result |
|---|---|---|
| `r47b_04` | Spa, still, 1280x720 window, 640x720 eyes | 54 calls and matrices, log and pixels IDENTICAL |
| `r47b_05` | Spa, the F2004's dash on its lap tape, mirror, 29 frames (all three recentrings) | 533, IDENTICAL in all 29 frames |
| `r47b_06` | Magione, chase, 4x MSAA (resolved into the swap chain), 7 frames | 159, IDENTICAL in all 7 frames |

In `rustyac.exe`: video.ini `[CAMERA] MODE=OCULUS` makes this camera when a signed runtime
starts a session with a headset (else the plain camera, with a note). The rest is as for OpenVR
(`OculusManager::update` is `ViveManager`'s): the cockpit and a recentring at the start, look
left plus look right or Ctrl+Space to recentre, and the runtime's own recentring request
honoured after each swap. The game never closes the Oculus runtime and neither does the port.
Not tried live: this PC has no Oculus runtime and no headset.

Not done here: post-processing in VR (`renderYebis`; refused as for OpenVR), OculusManager's
Rift audio device choice (oculus.ini `AUTOSELECT_RIFT_AUDIO_DEVICE`).

### Step 4: the HUD and apps in the headset

`crates/rustyac-render/src/gui3d.rs` is `ksgui::GUI`'s 3D mode: `init3DMode` (a colour target
the size of the window, content/gui/oculus_pointer.png), `preRender3D` (the pointer's speed,
the target bound and cleared), `renderVPointer` (the pointer, orange, larger while the mouse
moves fast) and `render3D`, the handler of the `3D_GUI_NODE` in the unblurred part of the scene:
in each eye's transparent pass the target goes on 24 quads, 140 degrees of a cylinder of 0.4 m
around the driver's eyes (car.ini `[GRAPHICS] DRIVEREYES`), 0.45 m high, turning with the car
body, not with the head. The flat GUI and HUD are not drawn (`isRenderingGui` off), so all the
apps and HUD the game would show flat (forms, the app bar, the HUD objects, the start lights)
are drawn into that target instead, with the window's size and the ordinary mouse. The node is
hidden while the rear-view mirror is drawn. Nothing places it differently: there is no setting.

The proof: in a VR run with a car the render oracle gives the game a GUI with what these
functions read (active, no forms, the app bar hidden, the pointer showing; the mouse moving by a
script), calls the game's `init3DMode`, adds the node with the game's own handler (the lambda of
`Sim::Sim`, `_Do_call` 0x140196a30), and per frame calls `preRender3D`, the oracle's HUD pass
(the gates, the pit marker, the start lights) and `renderVPointer` before the scene, as
`Sim::renderScene` does; the port does the same with `gui3d.rs`.

| case | what | result |
|---|---|---|
| `r47b_07` | OpenVR, Spa, the F2004's dash on its lap, start lights in the GUI, 29 frames | IDENTICAL in all 29 frames (log and pixels, both eyes) |
| `r47b_08` | the same on Oculus | IDENTICAL in all 29 frames |
| `r47b_02/03/05/06` | the earlier VR cases with a car, now with the GUI on its cylinder | all IDENTICAL again |
| `r22c_09`, `r22c_13`, `r22c_17` | flat cases with start lights, pit marker, gates (the HUD pass moved into a shared block) | IDENTICAL |

In `rustyac.exe` the headset GUI is made with the first car; its GUI (forms, app bar, apps) and
HUD go into the target, one frame late (docs/port/differences.md). Not done: `FormOpenVR` (the
OpenVR app with the supersampling and eye-distance spinners: openvr.ini can be edited by hand),
the apps and forms drawn in the oracle's VR runs (the GUI oracle's own sequences are flat; the
drawing code is the flat code, proven there).

### Step 5: the page

The page has no VR. Its graphics page keeps AC's Rendering Mode choice (Oculus Rift, OpenVR) for
`rustyac.exe` and the game, marked as not drawn by Full mode; with `MODE=OCULUS` or `MODE=OPENVR`
Full mode draws the plain camera, as before (both VR ports are left out of the wgpu build).

Could WebXR do it later? Mostly yes, with differences the game's code would not cover:
`navigator.xr.requestSession('immersive-vr')` gives per-eye views (a pose and a projection per
eye, as `GetEyeToHeadTransform` and `GetProjectionMatrix` do) and an XR frame loop in place of
`WaitGetPoses`; the eyes would be drawn with the same per-eye code (`render_no_pp`'s steps) into
the session's WebGPU or WebGL layer, and `gui3d.rs` would carry over unchanged. What WebXR lacks:
the game's own runtimes (so nothing there could be proven against acs.exe beyond the per-eye
drawing), a WebGPU XR binding in browsers today (WebGL layers only, while Full mode is wgpu),
seated recentring as `ResetSeatedZeroPose` (WebXR's `local` space resets on the browser's own
gesture), and the mirror window as the game draws it. Not tried.

## Task 47: where it stood at v0.47.0

- Task 47 is done and tagged v0.47.0: triple screens (desktop and the page's Full mode),
  TrackIR head tracking, Steam's start (name, id, DLC keys). Everything proven against the
  game's code with stand-ins; nothing tried on real triple screens, a real TrackIR or a real
  Steam session.
- Task 47c (left over): Steam achievements, stats, leaderboards, Friends Leaderboard rows and
  avatar (`SteamInterface`, with `--steam-achievements on` off by default); the Triple screen
  app (`TripleScreenManager`, its spinners writing triple_screen.ini, desktop and page); the
  triple paths of SessionLeaderboard, FuelLight, DriveThroughPenaltyAvatar,
  EndSessionDisplayer, SystemNotification and the name tags (`DriverNameDisplayer`).
- Proofs at the end: `run_all.sh --quick` 169 cases (156 in the first version of this line: a miscount), 0 different (47 min); `render` with
  `RENDER_ONLY='r47_0[1-3]'` 18/18 and `fresh` with `FRESH_PARTS=cameras` 25/25.
- Scratch: `re/scratch/task47/` (disassembly of the triple camera: `bvsp.txt`, `rp.txt`,
  `rccp.txt`, `cts_ctor.txt`; TrackIR: `onboard_update.txt`, `getoffsets.txt`; edit scripts;
  render oracle runs in `ro/`, Steam tapes in `steam/`, the page's triple frames in `page/`).

## 1. Audit

| Area | What acs.exe does | Settings / files | rustyAC |
|---|---|---|---|
| Triple screens: camera | `Sim::createCamera` 0x1401982e0 makes a `CameraTripleScreen` for video.ini `[CAMERA] MODE=TRIPLE`: three views side by side in one window, each in a third of it, turned to its screen, with its own off-centre projection | video.ini `[CAMERA] MODE`; Documents `cfg/triple_screen.ini` | **done, proven** (step 2) |
| Triple: bezel / angle / width / distance | the "Triple screen" app (`TripleScreenManager`, SystemApps) turns its spinners into the screens' points and writes triple_screen.ini when it closes | triple_screen.ini `[SPINNERS]` | missing (Task 47c); the file it writes is read |
| Triple: HUD and apps | `Sim::isInTripleScreenMode` moves the flags, the ESC menu, the setup screen and the replay bar into the centre third | — | **on now** (the ports were proven in Task 29-34 with a made-up "triple" input; the game now sets it) |
| Triple: HUD parts not ported for triple | SessionLeaderboard, FuelLight, DriveThroughPenaltyAvatar, EndSessionDisplayer and SystemNotification test the switch too | — | missing (Task 47c) |
| Triple: name tags | `DriverNameDisplayer` projects with the three screens' matrices | — | the port hides them in triple mode (Task 47c) |
| Triple: mirrors | no triple code: the mirror texture and the virtual mirror are the same | — | same (proven: r47_02 has the mirror) |
| TrackIR | `CameraOnBoard` owns a `TrackIR`: NPClient64.dll from the registry, signature check, window, profile 1002, data 0x77; `getOffsets` each frame: angles ÷ VALUE (default 500) in degrees, x/y/z × 1e-5 clamped to ±0.1; look keys off while it works | Documents `cfg/trackir.ini` `[TRACK_IR] VALUE` | **done, proven with a stand-in** (step 3) |
| FreeTrack / opentrack | only through the same NPClient64.dll | — | **done** with TrackIR (not tried with either) |
| Steam login | `SteamInit` 0x1401a7950: `SteamAPI_Init` (no Steam: the game does not start), persona name (log only), Steam ID → `playerGUID`, minidump crash guard | — | **done** (`steam.rs`; the minidump filter left out on purpose) |
| Steam DLC ownership | 11 `BIsDlcInstalled` checks, each adds a kn5 key; `KN5IO::load` refuses a version-6 kn5 whose key is not owned, `FolderEncrypter` a data.acd | — | **done** |
| Steam overlay / screenshots | no calls: Steam injects the overlay at `SteamAPI_Init`; no screenshots | — | Steam stays on while rustyAC runs, `SteamAPI_RunCallbacks` each frame (overlay not tried) |
| Steam achievements / stats / leaderboards / avatar | `SteamInterface` (66 functions) | — | missing (Task 47c) |

## 2. Triple screens

Ported: `CameraTripleScreen::CameraTripleScreen` 0x1402248e0, `beginVirtualScreenPass`
0x140224e30, `renderPass` 0x140225180, `Camera::renderCameraCustomPerspective` 0x14020f720,
`XMMatrixPerspectiveOffCenterRH` 0x14020ed30, `INIReader::getFloat2` 0x140235760
(`crates/rustyac-render/src/triple.rs`, `camera.rs`, `crates/rustyac-physics/src/data/ini.rs`).

- The camera reads Documents `cfg/triple_screen.ini`, `[LEFT_SCREEN]` `[CENTER_SCREEN]`
  `[RIGHT_SCREEN]`, `LEFT_POINT` and `RIGHT_POINT` each (x across, y ahead, from the eyes). Each
  screen's height is its width × window height / (window width × 1/3).
- Each third of the window: the camera turns about its up axis by `asin(dy / width)` of that
  screen, the projection goes through the screen's edges (its distance from the eyes is the
  near plane's scale), then the camera turns back. The sky of all three, the depth cleared, the
  opaque meshes of all three, then the transparent ones. While a screen draws, the window's
  width is a third.
- `CameraTripleScreen`'s constructor asks its `CameraForwardYebis` for no motion blur: no blur in
  triple mode, on both sides.
- Its other change is a handler that sets two YEBIS values (godrays length 1, vignetting 0):
  rustyAC has no YEBIS, so it has nothing to act on.
- A missing or short triple_screen.ini crashes the game (null token in `getVector2`); rustyAC
  stops with a message instead.
- The HUD's triple switch (`Sim::isInTripleScreenMode`) is now set from the camera.
- The page: the graphics page's "Rendering Mode: Triple Screen" writes the same video.ini key;
  Full mode then draws the three views on its one canvas, and the HUD switch follows. It needs
  the page's Documents `cfg/triple_screen.ini` (the page has no Triple screen app to write it:
  Task 47c). Proof: `full_frame` (Full mode's frame, natively) at 5760×1080 with MODE=TRIPLE:
  the kgl call log of Direct3D 11 and of the page's wgpu backend are the same (36,748 lines,
  three 1920-wide viewports per pass); pictures differ only as two devices do (mean 0.9/255,
  WARP against the GPU). wgpu on Direct3D 12 WARP crashes on this PC with or without triple
  (an old WARP fault, Task 38b). Not tried in a browser.

### Proof

`render_oracle compare --triple --size 5760x1080`: the game's own `CameraTripleScreen` (built by
address, video.ini `MODE=TRIPLE`, triple_screen.ini in the stand-in Documents) against the port.
Besides the command log and the pixels, both sides write each screen's view × projection and
camera matrix (`currentViewProj`, `currentCameraMatrix`, the matrices the name tags read).

| Case | What | Result |
|---|---|---|
| r47_01 | spa, chase view, no car | 6 matrices the same; 4537 calls, 822 draws; pixels IDENTICAL |
| r47_02 | spa, dash view, f2004, mirror 512, 58 frames | 6 matrices the same; log and pixels IDENTICAL in all 58 frames |
| r47_03 | magione, chase, e30, axis modes 1, 2, 0 | log and pixels IDENTICAL in all frames |

These run with the triple_screen.ini the game's app writes at its start values (the one on
Super's PC). Proven against the game's code with a stand-in window; nobody has looked at it on
three real screens.

How to turn it on: video.ini `[CAMERA] MODE=TRIPLE` and a window three screens wide (for
example `--width 5760 --height 1080`); `Documents\Assetto Corsa\cfg\triple_screen.ini`
must have the three screens (the game's "Triple screen" app writes it; the start values above
are a flat 1 m wide screen at 1 m, its sides turned 3°).

## 3. Head tracking (TrackIR)

Ported: `TrackIR::TrackIR` 0x1401cdfe0, `~TrackIR` 0x1401ce2a0, `TrackIR::getOffsets`
0x1401ce530, `getDllLocation` 0x1401ce340, `NPClient_Init` 0x1401ce740 and its wrappers, the
TrackIR branch of `CameraOnBoard::update` 0x1400c9ea0, `Camera::rotateRoll` 0x14020fb30
(`crates/rustyac-game/src/trackir.rs`, `DrivingCamera::head` in `render/scene.rs`; desktop only).

- At the start rustyac.exe looks for `NPClient64.dll` in the folder of the registry's
  `HKCU\Software\NaturalPoint\NATURALPOINT\NPClient Location` `Path`, checks its two signature
  strings, then registers the window, asks for data 0x77 and profile 1002, stops the cursor and
  starts the data. `Documents\Assetto Corsa\cfg\trackir.ini` `[TRACK_IR] VALUE` divides the
  angles (500 without the file; the game's own file says 100).
- Each cockpit frame: one poll. Heading, pitch (negated) and roll ÷ VALUE in degrees turn the
  camera (heading, then pitch, then roll, about its own axes); x (negated), y, z × 1e-5, each
  within ±0.1 m, move it along the turned axes. While the tracker works, the look keys (left,
  right, back) and the driver's eye pitch are not used, as in the game.
- No DLL, a wrong signature or a failed version query: no head tracking, as before.
- FreeTrack and opentrack work only through their own `NPClient64.dll` and registry key, as in
  the game. Nothing of NaturalPoint's is shipped.

How to turn it on: install TrackIR's software (or opentrack with its "freetrack 2.0 enhanced"
output, which writes the registry key and the DLL) and start rustyac.exe; the cockpit view
follows the head. `cfg/trackir.ini` `VALUE` sets how far.

### Proof

| Case | What | Result |
|---|---|---|
| `car_oracle start-camera-micro`, seeds 1 2 3 | the game's `CameraOnBoard::update` with its TrackIR on in half the segments (division 500, 100, 1 or random), its `NP_GetData` a stand-in giving random poses, failed polls and untracked frames, against the port's `DrivingCamera` | driving cameras IDENTICAL: 66,200 / 66,900 / 65,200 lines, 17,700 head-tracked frames |
| `trackir_init.tape`, 7 cases | the game's `TrackIR::TrackIR` and `~TrackIR` on a stand-in registry, `LoadLibraryA`, `GetProcAddress` and NPClient (no `NP_GetSignature`; a wrong signature; a failed version query; no trackir.ini; VALUE=100; cursor and data refusing with no `[TRACK_IR]`; VALUE=250) against the port's `NpClient` / `TrackIr` | IDENTICAL: 151 lines, every call and argument in order, `isValid` and `divisionValue` |

The registry and `LoadLibraryA` lines on the port's side come from its constants
(`REGISTRY_KEY`, `dll_path`); the NPClient calls are the port's own code. Both run in the
`cameras` part of `tools/proofs/fresh.sh`. Proven against the game's code with a stand-in
device; nobody has tried it with a real TrackIR or opentrack.

## 4. Steam

Ported: `SteamInit` 0x1401a7950 and `SteamShutdown` 0x1401a8380 (`crates/rustyac-game/src/steam.rs`),
`KN5IO::addDLCKey` 0x140214de0 with `KN5IO::load` 0x1402151a0's key check
(`crates/rustyac-content/src/kn5.rs`), `FolderEncrypter::decryptFile` 0x14023bdd0's key check
(`crates/rustyac-content/src/acd.rs`).

- rustyac.exe loads the `steam_api64.dll` of the player's Assetto Corsa folder. When Steam runs
  (`SteamAPI_IsSteamRunning`) and `SteamAPI_Init` works (logged in, owns the game), it does what
  `SteamInit` does: persona name, Steam id twice, then the 11 DLC checks in the game's order
  (app ids 347990, 404430, 423630, 467980, 493190, 509350, 526060, 540710, 540711, 619680,
  675590). Steam stays on until the program ends (`SteamShutdown`), with
  `SteamAPI_RunCallbacks` each frame as `SteamInterface::update` does.
- The Steam id is the online GUID (as before) and the persona name the default driver name
  when none is given.
- With Steam on, a kn5 (version 6 and up) or data.acd whose DLC key is not owned is refused,
  as the game refuses it. In this install 207 kn5 files and 37 data.acd carry a key (four
  keys: 0x3b787, 0xa4691, 0xb5735, 0xdebda; mod cars reuse 0x3b787 and 0x905f2). The counts
  include the Japanese Pack cars; the proof does not enumerate cars, so its case count did not
  grow.
- Without Steam (not running, `--no-steam`, or a proof run with `RUSTYAC_PROOF_RUN`) nothing
  changes: no key is checked, everything loads, the id is rustyac_net.ini's.
- Left out: `SetUnhandledExceptionFilter(MiniDumpFunction)` (crash dumps to the game's makers
  through Steam), the game's refusal to start without Steam.
- Not ported (Task 47c): `SteamInterface`: achievements, stats, leaderboards, the Friends
  Leaderboard rows and the avatar. Their checks read the whole race (cars, laps, assists,
  session, mode), so the game side of a proof needs a running `Sim`; there is no
  `--steam-achievements` switch yet, because nothing would be behind it.

### Proof

`car_oracle steam-micro --seed n` runs the game's `SteamInit` and `SteamShutdown` on a stand-in
`steam_api64.dll` (tools/car_oracle/src/game/steam.rs) that writes down every call and its
arguments, then the GUID and both key vectors. `steam_check` (crates/rustyac-game/examples)
runs the port on the same stand-in answers. 64 cases a seed: Steam missing, no DLC owned, all
owned, made-up mixes; names with accents, empty, long; crash guard on and off.

| Seed | Lines | Steam calls | Result |
|---|---|---|---|
| 1 | 2275 | 1891 | IDENTICAL |
| 2 | 2275 | 1891 | IDENTICAL |
| 3 | 2275 | 1891 | IDENTICAL |

A tape with one app id changed is caught (DIFFERENT at line 19). The key refusal itself has a
unit test (`dlc_keys_refuse_only_unowned`). Proven with stand-ins only: the real Steam client,
its overlay inside rustyAC's window and a real account's DLC answers were not tried (Super's
account must not be touched by a proof).

### How to turn it on

Start Steam and log in; run rustyac.exe as usual. `--no-steam` keeps Steam out.
