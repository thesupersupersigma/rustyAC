# Triple screens, TrackIR and Steam (Task 47, v0.47.0)

## Resume here

- Done: step 1 (audit), step 2 (triple screen camera, HUD switch, render oracle `--triple`),
  step 3 (TrackIR: `crates/rustyac-game/src/trackir.rs`, the cockpit camera's `head`, proven in
  the `cameras` part of `tools/proofs/fresh.sh`).
- Next: step 4 (Steam stand-in).
- Scratch: `re/scratch/task47/` (disassembly of the triple camera: `bvsp.txt`, `rp.txt`,
  `rccp.txt`, `cts_ctor.txt`; edit scripts; render oracle runs in `ro/`).

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
| Steam login | `SteamInit` 0x1401a7950: `SteamAPI_Init` (no Steam: the game does not start), persona name (log only), Steam ID → `playerGUID`, minidump crash guard | — | partly (identity.rs) |
| Steam DLC ownership | 11 `BIsDlcInstalled` checks, each adds a kn5 key; `KN5IO::load` refuses a version-6 kn5 whose key is not owned | — | step 4 |
| Steam overlay / screenshots | no calls: Steam injects the overlay at `SteamAPI_Init`; no screenshots | — | step 4 |
| Steam achievements / stats / leaderboards / avatar | `SteamInterface` (66 functions) | — | step 4 |

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
- The page draws one view (no triple mode in the page yet: Task 47c).

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
