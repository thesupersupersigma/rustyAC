# Camera

Source: `acs.exe` + `acs.pdb` through the local index (`re/decomp`, `re/types`, `tools/re_query.py`, `tools/disasm.py`),
plus a read-only look at the camera ini files in `system/cfg`, the Documents `cfg` folder, one track and the SDK example
car. Shallow atlas pass: constructors, `update` entry points and file names. Nothing was modified.
"Confirmed" = read in the pseudo-C / call index / disassembly. "Interpretation" is marked as such.

## 1. What it is

There is exactly one camera that the renderer draws from: `Sim::sceneCamera`, a `CameraForward` (or one of its
subclasses, see renderer.md). Everything in this area only decides, every frame, *where that camera is, where it looks,
its field of view, exposure, depth of field and shadow ranges*.
`ACCameraManager` owns one small "camera mode" object per view type (cockpit, chase/bonnet/bumper/dash, car-mounted,
track TV, helicopter, orbit, free, start sequence). Each of them is a `GameObject` holding a pointer to the same scene
camera. Each frame the manager switches exactly one of them on; that one's `update` overwrites the scene camera's
matrix and lens values, and also moves the audio listener.
The cockpit camera adds the "driver's head": a positional offset proportional to the car's G forces with a lag filter,
a sine-wave shake that grows with speed, glance left/right/back, optional world-aligned horizon, and TrackIR head
tracking. In VR the headset pose is applied later by the stereo camera class, on top of this matrix.
Track cameras come from the track's `data/cameras.ini` (sets of TV cameras with in/out points along the lap); the
camera whose range contains the focused car is picked and aims at the car, zooming between a min and max FOV.
Car-mounted cameras come from the car's `data/cameras.ini`; the driver's eye point from `car.ini`.
F1-F7 select the mode (`Sim::onKeyDown`), with a short fade between modes; a "random" mode hops between modes on a timer.

## 2. Main classes

| Class | Source / library | Size | Role |
|---|---|---|---|
| `Camera` | Camera.obj / KunosSim.lib | 0x210 | Base: world matrix, fov, near/far, `getViewMatrix`, `getPerspectiveMatrix`, move/rotate helpers. Parent of `CameraShadowMapped` -> `CameraForward` (renderer.md) |
| `ACCameraManager` | acs.exe | 0x198 | Owns all mode objects; current `CameraMode` (`eCockpit`=0, `eCar`=1, `eDrivable`, `eTrack`=3, `eHelicopter`, `eOnBoardFree`=5, `eFree`, `eStart`); fade in/out; random mode; remembers last mode in `camera_manager.ini` |
| `CameraOnBoard` | acs.exe | 0x170 | Cockpit view: driver eyes, G-force head offset, shake, glance, FOV, `TrackIR` member |
| `CameraDrivableManager` | acs.exe | 0x130 | The other driving views: `eChase`, `eChase2`, `eBonnet`, `eBumper`, `eDash` |
| `CameraCarManager` | acs.exe | 0x70 | Fixed cameras bolted to the car (list of `CameraCarDefinition`: matrix, fov, exposure, externalSound) |
| `CameraTrack` | acs.exe | 0xd8 | Track TV cameras; holds several `CameraManager` sets, picks the active `ACCamera`, aims and zooms |
| `CameraManager`, `ACCamera` | ksACCamera.lib | 0x48 / 0xf8 | One camera set loaded from a `cameras.ini`; one TV camera (position, fov range, in/out point, shadow splits, exposure, DOF, optional spline path) |
| `CameraHelicopter` | acs.exe | 0x78 | Simple view from above following the car |
| `CameraOnBoardFree` | acs.exe | 0x1b0 | Orbit camera around the car (F5), spherical coordinates, own fov / DOF / exposure |
| `CameraMouseControl` -> `CameraMouseControlBase` | ksGame.lib / KunosSim.lib | 0xb0 / 0x40 | Free-flying camera with mouse look + keyboard (F7) |
| `CinematicFreeCamera` | acs.exe | 0x130 | Smoothed free camera for video editing (replaces mouse control when enabled in `video_editing.ini`) |
| `CameraModeStart` | acs.exe | 0x100 | Pre-race sequence that cuts between cars on the grid |
| `TrackIR` + `NPClientWraps` | acs.exe | 0x8 | NaturalPoint head tracking: loads `NPClient64.dll` at run time, returns yaw/pitch/roll and position |
| `DriverEyesPositioner` | SystemApps.lib | 0x290 | In-game app "Onboard settings": moves the eye point, pitch and FOV, saves them |
| `SignalGenerator`, `SinSignalGenerator` (`SignalGenerator3D<>`) | KunosSim.lib | - | Sine generators behind the camera shake (also used by `Tyre` and `GearShiftShake`) |
| `CameraFacing` | acs.exe | 0x58 | **Not a camera**: camera-facing crowd billboards of a track (`data/camera_facing.ini`), built by `TrackAvatar` on `StaticParticleSystem` |
| `ImageGeneratorCamera` | acs.exe | 0x158 | Empty shell, member of `ACCameraManager`; counted in renderer.md |

## 3. Entry points

**Creation.** `Sim::Sim` 0x140192070 calls `Sim::createCamera` 0x1401982e0 (creates the scene camera, renderer.md) and
then `ACCameraManager::ACCameraManager` 0x140031270, which constructs every mode object as a child game object:
`CameraOnBoard` 0x1400c84e0 (which builds `TrackIR::TrackIR` 0x1401cdfe0 -> `NPClient_Init` 0x1401ce740),
`CameraDrivableManager` 0x1400c4a80 (`initChaseCamFromINI` 0x1400c61c0), `CameraCarManager` 0x1400c4470,
`CameraTrack` 0x1400cab20, `CameraHelicopter` 0x1400c8010, `CameraOnBoardFree` 0x140115100, `CameraModeStart`
0x14005f940, and either `CameraMouseControl` 0x14023fda0 or `CinematicFreeCamera` 0x14006c010.
Car-mounted cameras are read per car by `CarAvatar::initCameraCar` 0x1400d47c0. `DriverEyesPositioner` 0x14030fa60 is
created by `Sim::Sim` like the other apps.

**Per frame** (main thread, inside `Game::update` 0x140243010, before any rendering):

1. `ACCameraManager::update` 0x1400345e0: clears the active flag of all children, steps the random mode
   (`stepCameraRandom` 0x140034300), then sets the flag of the one object that matches `mode`.
2. The active object's `update` writes into the shared scene camera:
   `CameraOnBoard::update` 0x1400c9ea0, `CameraDrivableManager::update` 0x1400c6a30 (dispatches to `updateChase`
   0x1400c7120, `updateBonnet` 0x1400c6ba0, `updateBumper` 0x1400c6df0, `updateDash` 0x1400c7b50, then `updateLook`
   0x1400c7de0), `CameraCarManager::update` 0x1400c47d0, `CameraTrack::update` 0x1400cbe30,
   `CameraHelicopter::update` 0x1400c80e0, `CameraOnBoardFree::update` 0x140115db0 (`updateOrbit` 0x140116310),
   `CameraMouseControl::update` 0x140240040 (`CameraMouseControlBase::mouseLook` 0x14020fcc0, `stepKeyboard`
   0x14020fe00), `CinematicFreeCamera::update` 0x14006d980, `CameraModeStart::update` 0x140060420 (`updateCar`
   0x140060470).
   What they set (confirmed for on-board, car and track): world matrix, fov, min/max exposure, DOF values, radial-blur
   speed, shadow cascade ranges (`CameraShadowMapped::setShadowMapsSplits` 0x14020d5f0) and `AudioEngine::setListener`.
3. Later in the same frame `Sim::renderScene` 0x14019e570 calls `sceneCamera->render(...)`; the renderer reads the
   matrix through `Camera::getViewMatrix` 0x14020eff0 / `getPerspectiveMatrix` 0x14020ef00 in `Camera::renderCamera`
   0x14020f5d0. `Sim::getSceneCamera` 0x140199630 is the accessor other code uses.

**Mode switching.** `ACCameraManager::setMode` 0x1400340d0 (mode, force, random). Keys, in `Sim::onKeyDown`
0x14019a940 (confirmed from the switch and the lambdas' disassembly):

| Key | Action |
|---|---|
| F1 | `setMode(eCockpit)`: cockpit; pressed again cycles through the drivable views (`CameraDrivableManager::nextMode`) and back |
| F2 | fade, then `setMode(eTrack)` and `randomMode = true` (random camera, timings from `random_camera.ini`) |
| F3 | fade, then `setMode(eTrack)`; pressed again `CameraTrack::nextSet` 0x1400cbc90 (next camera set) |
| F4 | only with 2+ cars: focus back on car 0 |
| F5 | fade, then `setMode(eOnBoardFree)` (orbit) |
| F6 | `setMode(eCar)`; if already there `CameraCarManager::nextCamera` 0x1400c4710 |
| F7 | `setMode(eFree)`, only when the free camera is allowed |
| F8 | screenshot (renderer.md) |
| Numpad + / - | on-board FOV +/- 0.5 through `CameraOnBoard::setPlayerFov` 0x14019e7b0 |

Fades go through `ACCameraManager::fadeIn` 0x140033690 / `registerToFadeInEvent` 0x140033ce0: the mode change runs in
a callback when the screen is black. `setDrivableCarIndex` 0x140033fd0 and `Sim::nextCar` 0x14019a740 /
`previousCar` 0x14019e180 change which car is followed. `loadPreviousSessionUserCamera` 0x1400337e0 restores the last
mode at start.

**Head movement (confirmed in `CameraOnBoard::update`).** Offset = `gForceMix` (x,y,z) x car `accG`, low-passed with
`gForceLagMix`; shake = `SignalGenerator3D<SinSignalGenerator>` with frequency and amplitude scaled by `speed * 0.01`;
both multiplied by the car's `SHAKE_MUL` and the user's `SHAKEMULT`. `lookLeftRight` 0x1400c9b80 animates
`headRotationAnimation` towards `HEAD_MAX_DEGREES` at `SPEED`; `lookBack` 0x1400c9a60 jumps to a look-back point.
`TrackIR::getOffsets` 0x1401ce530 returns yaw / pitch / roll divided by `divisionValue` plus x/y/z, applied with
`Camera::rotateHeading` 0x14020f940 / `rotatePitch` 0x14020fa80 / `rotateRoll` 0x14020fb30. The shake is skipped in VR
unless `assetto_corsa.ini [VR] ENABLE_CAMERA_SHAKE` is set.

**Track cameras.** `CameraTrack::loadSet` 0x1400cb670 -> `CameraManager::load` 0x1401f48e0 (one `ACCamera` 0x1401f4240
per `[CAMERA_n]`); `CameraManager::getActiveCamera` 0x1401f4820 picks by the car's lap position against `IN_POINT` /
`OUT_POINT`; `CameraTrack::update` aims with `mat44f::createTarget`, follows an optional spline and adds a slow wobble.

## 4. What it reads from disk

| Path (as built in code) | Keys | Read by |
|---|---|---|
| `content/tracks/<track>[/<layout>]/data/cameras.ini`, `cameras_<n>.ini` (further sets) | `[HEADER] VERSION, CAMERA_COUNT, SET_NAME`; `[CAMERA_n] NAME, POSITION, FORWARD, UP, MIN_FOV, MAX_FOV, IN_POINT, OUT_POINT, SHADOW_SPLIT0..2, NEAR_PLANE, FAR_PLANE, MIN_EXPOSURE, MAX_EXPOSURE, DOF_FACTOR, DOF_FOCUS, DOF_RANGE, DOF_MANUAL, SPLINE, SPLINE_ROTATION, SPLINE_ANIMATION_LENGTH, FOV_GAMMA, IS_FIXED` | `CameraTrack::loadSet`, `CameraManager::load` |
| `content/tracks/<track>/data/<spline>.csv` named by `SPLINE=` | camera path | `CameraManager::load` |
| `content/tracks/<track>/data/crew.ini` | `[HEADER] SIDE` | `CameraModeStart` |
| `content/cars/<car>/data/cameras.ini` | `[CAMERA_n] POSITION, FORWARD, UP, FOV, EXPOSURE, EXTERNAL_SOUND` | `CarAvatar::initCameraCar` |
| `content/cars/<car>/data/car.ini` `[GRAPHICS]` | `DRIVEREYES, ON_BOARD_PITCH_ANGLE, SHAKE_MUL, ON_BOARD_LOOKBACK_OFFSET, BUMPER_CAMERA_POS, BUMPER_CAMERA_PITCH, BONNET_CAMERA_POS, BONNET_CAMERA_PITCH, DRIVABLE_LOOKBACK_OFFSET` | `CameraOnBoard`, `CameraDrivableManager`, `CarAvatar::init3D` |
| `content/cars/<car>/data/dash_cam.ini` | `[DASH_CAM] POS, EXP` | `CameraDrivableManager` |
| Documents `cfg/cars/<car>/view.ini` (user override, read and written) | `[DRIVER_EYES_POSITION] DRIVEREYES`, `[CAMERA] ON_BOARD_PITCH_ANGLE`, `[DASH_CAM] POS` | `CameraOnBoard`, `CarAvatar::init3D`, `DriverEyesPositioner::saveSettings` 0x140313920 |
| Documents `cfg/camera_onboard.ini` | `[MODE] IS_WORLD_ALIGNED, FOV`; `[GFORCES] MIX, LAG`; `[SHAKE] SCALE, FREQ, RANDOM`; `[ROTATION] HEAD_MAX_DEGREES, SPEED` | `CameraOnBoard`, `CameraDrivableManager`, `DriverEyesPositioner` |
| Documents `cfg/camera_manager.ini` (read and written) | `[LAST_CAMERA] MODE, DRIVABLE_MODE`; `[FADE] COLOR`; `[SHAKE] GFORCEX, GFORCEY, GFORCEZ, SHAKEMULT` | `ACCameraManager`, `CameraOnBoard`, `CameraDrivableManager` |
| Documents `cfg/video_editing.ini` | `[CINEMATIC_CAMERAS] ACTIVE, MOUSE`; `[CAMERA_SETTINGS] MOVE_SPEED, MIN_MULT, MAX_MULT, FOV, MOVEMENT_DELAY, MOUSE_LOOK_SPEED` | `ACCameraManager`, `CinematicFreeCamera::loadSettingsFromINI` 0x14006cf60 |
| Documents `cfg/trackir.ini` | `[TRACK_IR] VALUE` | `TrackIR` |
| `system/cfg/camera_drivable.ini` | `[ROTATION] MAX_DEGREES, SPEED` | `CameraDrivableManager` |
| `system/cfg/chase_cam.ini` | `[CHASE_0]`, `[CHASE_1]`: `DISTANCE, HEIGHT, PITCH` | `initChaseCamFromINI` |
| `system/cfg/camera_track.ini` | `[CAMERA_SETTINGS] DISTANCE_SCALE, UPWARD_OFFSET, CAMERA_WOBBLING_SPEED, CAMERA_WOBBLING_STRENGTH` | `CameraTrack`, `CameraMouseControl`, `CinematicFreeCamera` |
| `system/cfg/camera_onboard_free.ini` | `[CAMERA_SETTINGS] ROTATION_SPEED, ESC_DOF_FACTOR, MIN_MULT, MAX_MULT, FOV, EXPOSURE, SPHERICAL_COORDS` | `CameraOnBoardFree` |
| `system/cfg/random_camera.ini` | `[SETTINGS] MINIMUM_TIME, MAXIMUM_TIME`; `[CAMERA_TRACK]`, `[CAMERA_COCKPIT]`, `[CAMERA_DRIVABLE]`, `[CAMERA_CAR]`: `PROBABILITY` | `ACCameraManager` |
| `system/cfg/assetto_corsa.ini` | `[CAMERA] ALLOW_FREE_CAMERA`; `[VR] ENABLE_CAMERA_SHAKE` | `Sim::Sim`, `CameraOnBoard::update` |
| `content/cars/<car>/ui/fade_texture.png`, `content/gui/fade/fade_default.png` | fade image | `ACCameraManager` |
| Registry `HKCU\Software\NaturalPoint\NATURALPOINT\NPClient Location` -> `NPClient64.dll` | TrackIR runtime | `getDllLocation` 0x1401ce340 |
| `content/tracks/<track>/data/camera_facing.ini` | `[CAMERA_FACING_n] SURFACE, ELEMENTS, SIZE, TEXTURE, TEXTURE_ROWS, TEXTURE_COLUMNS, SHADED, DIFFUSE, AMBIENT` | `CameraFacing::CameraFacing` 0x14005e640 |

Written by the in-game tools: `view.ini`, `camera_onboard.ini` (`DriverEyesPositioner`), the car's `car.ini` /
`dash_cam.ini` (`saveToINI` 0x1403144e0, `saveToDashINI` 0x140314120, developer use), `system/cfg/chase_cam.ini` and the
car/track `cameras.ini` (the `FormCamera` developer app in SystemApps.lib, not counted here).

## 5. Size

`python tools/re_query.py size <stems>`: **296 hand-written functions, 123,502 code bytes, 20 object files**.

| Object | Functions | Code bytes | | Object | Functions | Code bytes |
|---|---|---|---|---|---|---|
| DriverEyesPositioner | 37 | 23,094 | | ACCameraManager | 32 | 15,854 |
| CameraDrivableManager | 23 | 15,254 | | CameraOnBoard | 20 | 11,231 |
| CameraTrack | 20 | 8,772 | | CameraFacing | 10 | 8,495 |
| CinematicFreeCamera | 26 | 8,051 | | CameraOnBoardFree | 20 | 6,819 |
| CameraManager (ksACCamera.lib) | 6 | 6,151 | | CameraModeStart | 18 | 5,153 |
| Camera (KunosSim.lib) | 21 | 4,513 | | NPClientWraps | 10 | 2,795 |
| TrackIR | 6 | 1,901 | | CameraCarManager | 14 | 1,715 |
| CameraMouseControl (ksGame.lib) | 4 | 1,063 | | CameraHelicopter | 10 | 963 |
| CameraMouseControlBase | 7 | 804 | | ACCamera (ksACCamera.lib) | 2 | 503 |
| SinSignalGenerator | 4 | 186 | | SignalGenerator | 6 | 185 |

Not counted here: `CameraShadowMapped`, `CameraForward`, `CameraForwardYebis`, `CameraMirror`, `CameraMeshFilter`,
`CameraTripleScreen`, the stereo cameras and `ImageGeneratorCamera` (renderer.md); `FormCamera`, `TripleScreenManager`,
`PhotoMode` (SystemApps.lib UI apps); `Sim::onKeyDown` (sim.obj, main_loop.md); `GearShiftShake` (renderer.md).

## 6. Port difficulty: **M**

- Each mode is a small, self-contained piece of vector maths (S each); the whole area is 124 kB, of which 23 kB is the
  eye-position app (UI) and 8.5 kB is the crowd billboard class that is not a camera at all.
- No third-party library is needed. Matrix/quaternion maths: `glam`. ini files: the project's own reader.
- TrackIR: skip at first; later either load `NPClient64.dll` through `libloading` with the same ten `NP_*` calls or use
  an OpenTrack-style UDP input. VR head pose belongs to the renderer side (`openxr`).
- Faithful feel matters more than bit-exactness here: the G-force lag filter, shake scaling by speed and the chase-cam
  spring are frame-rate dependent in the original (they run once per rendered frame with `dt`), so copy the formulas.
- Depends on: the scene camera object (renderer), `CarAvatar` physics-state snapshot (position, velocity, `accG`,
  speed), the track spline position of the focused car (for TV cameras), input (keys, mouse), audio listener, the
  `GameObject` tree, replay (cameras also run during replays; `CameraTrack::update` checks `isInReplaymode`).
- Order suggestion: `Camera` base + cockpit view first (needed to drive), then chase, then track TV cameras, rest later.

## 7. Open questions

- **`CameraFacing` is counted here only because it was assigned to this area by name.** It is a track visual (crowd
  billboards) and would fit renderer.md or track.md better; move the 10 functions / 8,495 bytes if the index is tidied.
- `SignalGenerator` / `SinSignalGenerator` are shared with `Tyre` (physics) and `GearShiftShake`; counted here because
  the camera shake is their main user in the game layer. `GearShiftShake` (shakes the gear-lever mesh, not the camera)
  is counted in renderer.md.
- `system/cfg/camera_free.ini` and `system/cfg/camera_start_mode.ini` exist on disk but no string in the executable
  refers to them; `CameraMouseControl` and `CinematicFreeCamera` read `camera_track.ini` / `video_editing.ini` instead.
  They look like leftovers. Not verified beyond the string index.
- Numeric values of `eDrivable`, `eHelicopter`, `eFree`, `eStart` in `CameraMode` were not read (only 0, 1, 3 and 5 are
  confirmed from immediates); the PDB enum was not dumped. `[CAMERA_HELICOPTER] PROBABILITY` exists in
  `random_camera.ini` but only four probability sections appear in the constructor's strings.
- No steering-based "look ahead into the corner" was found at this depth: the cockpit view has G-force offset, shake,
  glance keys and TrackIR only. `updateLook` in the drivable views and the chase-camera follow maths
  (`updateChase`, 2.6 kB) were not read.
- How `CameraTrack` zooms between `MIN_FOV` and `MAX_FOV` (`FOV_GAMMA`, distance ranges from
  `CameraManager::initCamerasDistanceRanges` 0x1401f48b0) and how DOF values reach YEBIS were not traced.
- F7 (free camera) is gated by a boolean on `Sim`. `Sim::Sim` reads `[CAMERA] ALLOW_FREE_CAMERA` (the key exists in
  `system/cfg/assetto_corsa.ini`, default 0); that this key is the F7 gate is an interpretation, the member was not traced.
