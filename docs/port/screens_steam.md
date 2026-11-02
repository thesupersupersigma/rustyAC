# Triple screens, TrackIR, Steam and VR (Tasks 47 and 47b)

## Task 47b: VR

### Resume here

- Step 0 done: no proof case vanished (below); `run_all.sh` now prints vanished cases.
- Step 1 done: the audit table below.
- Step 2 done (camera): `rustyac-render` `vr.rs` is the game's `StereoCameraVive`, proven on a stand-in OpenVR runtime (render cases `r47b_01..03`). `rustyac.exe` uses it with video.ini `MODE=OPENVR`. Step 3 next.
- Step 3 camera done: `oculus.rs` is the game's `StereoCameraForward`, proven on a stand-in Oculus runtime (render cases `r47b_04..06`). Next: `rustyac.exe` with `MODE=OCULUS`, then step 4 (the HUD in the headset).

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

Not done here: post-processing in VR (`renderYebis`; refused as for OpenVR), OculusManager's
Rift audio device choice (oculus.ini `AUTOSELECT_RIFT_AUDIO_DEVICE`).

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
