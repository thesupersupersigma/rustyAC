# VR and screens

This area is everything CSP does to *where and how the finished picture goes out of the game*: the three VR back
ends (Oculus, OpenVR and CSP's own OpenXR, which is wired in underneath acs.exe's OpenVR camera), the triple-screen
camera tweaks, the custom render modes (360, fisheye, split screen) and the window / DPI / swap-chain / V-sync / VRR
layer (DXGI Tweaks). On this PC (one Odyssey monitor + the MacBook panel, no headset, `MODE=DEFAULT`) none of the VR,
triple or custom-mode code runs; only the DPI, window-placement and swap-chain parts do.

Address conventions: `dwrite.dll RVA 0x...` (image base 0x180000000); acs.exe addresses are the 0x14... VAs of
`docs/atlas`. Category letters as in `physics_car_hooks.md`: A = pass-through that records state, B = wraps the
original and changes inputs / outputs, C = replaces (original not called, or only conditionally), D = lookup.
Sizes of dwrite.dll functions are the exception-table sizes (`dll_funcs.tsv`); acs.exe sizes are from
`acs_index/functions.tsv`.

## 1. What it does

### 1.1 The render-mode map (verified)

One table inside CSP names every render mode (`dwrite.dll RVA 0xcf3410`, 1,716 B; strings `0x2268018`..`0x22680e8`,
UTF-16): `simple`, `triple`, `vr.oculus`, `vr.openvr`, `custom.360`, `custom.fisheye`, `custom.splitscreen`,
`custom.comparison`, `unknown` `[dll]`. There is **no `vr.openxr` entry**: OpenXR is a sub-case of `vr.openvr`
(see 1.2). `custom.comparison` is not documented anywhere in the shipped configs (see section 8). The Render Stats
app's performance report uses a second list for its "Render mode" line: "VR (Oculus)", "VR (OpenXR)", "VR (OpenVR)",
"triple screen", "fisheye", "split screen", "comparison" (`dwrite.dll RVA 0x223b7d0`..`0x223b930`, in the
function at `0x7a3010`, 18,512 B) `[dll]`. Apps use the same classes in their manifests: `DISABLED_RENDER_SINGLE`,
`..._TRIPLE`, `..._VR`, `..._CUSTOM`, `HIDDEN_RENDER_VR`, `HIDDEN_RENDER_CUSTOM` (strings `0x2267fb0`..`0x2268180`)
`[dll][lua]` (see `lua.md`, `ui_apps.md`).

How a mode gets chosen (acs.exe side): `Sim::createCamera` (0x1401982e0) reads video.ini `[CAMERA] MODE` =
`OCULUS` / `OPENVR` / `TRIPLE` / other (`docs/atlas/renderer.md`; acs.exe string "OPENVR" is in createCamera)
`[prior]`. CSP's own selectors are written by Content Manager: `data_manifest.ini [CUSTOM_RENDER_MODES]` lists the
extra modes the launcher offers: `__EXT_OPENXR = OpenXR`, `__EXT_360 = 360°`, `__EXT_FISHEYE = Fisheye`,
`__EXT_SPLITSCREEN = Split screen, custom_rendering_modes.ini:MODE_SPLITSCREEN/ALT_CONTROLS_CONFIG` and
`[FEATURES] CUSTOM_RENDERING_MODES=1`, `INCLUSIVE_FULLSCREEN=! dxgi_tweaks.ini:COMPATIBILITY/EXCLUSIVE_FULLSCREEN`
`[ini]`. CSP reads the choice as `__CM_CUSTOM_MODE` (string `0x226a0b0`, read in `0x11e82b0` and `0x1387350`) `[dll]`.

### 1.2 VR

**Three back ends, one camera class.** acs.exe has two VR cameras: `StereoCameraForward` (Oculus, LibOVR shim linked
into acs.exe) and `StereoCameraVive` (OpenVR). CSP hooks both (26 hook rows, section 2.1) and **replaces most of their
render path** with one shared, single-pass-stereo capable implementation:

* **Oculus** (LibOVR 1.x): the acs.exe shim `OVR_CAPIShim.obj` (19 `ovr_*` thunks plus the DLL loader helpers,
  0x1401dc4b0..0x1401dddd0; `OVR_CAPI_Util.obj` adds `ovr_CalcEyePoses`, `ovr_GetEyePoses`, `ovrMatrix4f_Projection`)
  is kept for init / tracking / swap chains; CSP additionally looks up `ovr_WaitToBeginFrame`, `ovr_BeginFrame`,
  `ovr_EndFrame`, `ovr_GetFovStencil`, `ovr_GetTrackingState` (hooked, `0x6de950`), `ovr_SetControllerVibration`,
  `ovr_GetInputState`, `ovr_GetPredictedDisplayTime`, `ovr_GetTimeInSeconds`, `ovr_Shutdown` through the
  `...Ptr` names and from the runtime DLL itself (error "Failed to find Oculus function%ls:%ls", "Make sure your
  Oculus runtime is up-to-date"; `LibOVRRT` string at `0x2291ae0`) `[dll]`. Its present function (`0x11c7980`,
  2,041 B, profiler label "VR present") calls WaitToBeginFrame / BeginFrame / EndFrame with a depth layer for
  ASW 2.0 ("Failed to create Oculus AWS2 depth texture", `ovrTimewarpProjectionDesc_FromProjection`) `[dll]`.
  `OCULUS_TWEAKS UPDATE_MANIFEST` writes the Oculus store manifest so the Oculus UI shows AC's name and icon
  (`0xe930a0`, 4,360 B; strings "Oculus/CoreData/Manifests", "cover_landscape_image_large.png") `[dll]`.
* **OpenVR**: acs.exe asks `openvr_api.dll` for interface versions `IVRSystem_015`, `IVRCompositor_020`,
  `IVRSettings_002` (acs.exe strings in `StereoCameraVive` / `vr::VRCompositor` / `vr::VRSettings`, `acs_index`)
  `[dll]`. CSP's frame loop submits eye textures through `IVRCompositor::Submit` (vtable slot 5) with the
  "texture with depth" flag when `SUBMIT_DEPTH` is on, `PostPresentHandoff` (slot 7) in submit mode 3, and polls
  `IVRSystem::PollNextEvent`: event types 700 and 704 (Quit, DriverRequestedQuit) make AC exit
  (`StereoCameraVive::UpdateHMDMatrixPose` hook `0x11ca710`) `[dll][interp]`. For OpenComposite (an OpenXR-based
  `openvr_api.dll` replacement; detected by `0x6db240`, 578 B, strings "OpenComposite", "system/x64/openvr_api.dll",
  "ipdOffset") CSP forces the seated tracking space each frame (profiler label "Tracking space fix") and wraps
  `IVRSettings` (hook on `vr::VRSettings`, `0x11ca4d0`) `[dll][interp]`.
* **OpenXR (CSP's own)**: not in acs.exe. CSP exports a complete **OpenVR shim**: the six `\openvr_api\\VR_*`
  names are hooked (replacement RVAs verified from the registration code, section 2.3) and answer
  `IVRSystem_*`, `IVRCompositor_*`, `IVRSettings_*` requests with objects backed by an OpenXR session. acs.exe
  therefore still constructs `StereoCameraVive` (log string `0x2297bf0`: "OpenXR requested; ACS will construct
  StereoCameraVive"; Lua SDK: `isOpenXRMode` = "native OpenXR path is used (ACS still constructs OpenVR stereo
  camera)") `[dll][sdk]`. The OpenXR code is one cluster of **142 functions / 79,360 bytes at `0x6e0060..0x6f4000`**
  (`dll_funcs.tsv`) plus `0x6c10d0` (adapter pick) and `0x6dc500..0x6e0060` (27 functions, 14,825 B: hidden-area
  mesh, VR cover and controller debug shapes) `[dll]`. It loads `openxr_loader.dll` (UTF-16 string `0x222b9b0`;
  shipped as `extension/internal/plugins/openxr_loader.dll`, 2,678,296 B) and honours the environment variable
  `XR_RUNTIME_JSON` and the key `RUNTIME_JSON` `[dll][ini]`.

**Frame (Vive path, `StereoCameraVive::render` replacement `0x11cb110`, 839 B, category C).** Profiler labels in the
order seen: "VR mirroring" (mirror texture), `UpdateHMDMatrixPose` (only when `SUBMIT_FIRST` < 2; its label "VR
synchronization" is where `WaitGetPoses`-style blocking happens), shadow / main pass ("Main pass", "Bind
shadows"), per-eye render through the shared single-pass-stereo code, `finishRendering` ("VR present") `[dll]`.
`normalRendering` (both cameras, `0x11c6d80` / `0x11c9520`, 8,653 / 8,650 B of pseudo-C, identical except one
field offset) is fully re-implemented: it picks the single-pass-stereo state (global byte `0x182693970` = SPS
active, dword `0x18269397c` = current eye, per-eye constant data at `0x1826939f0 + eye*0x220`), draws the scene
once with VS instancing or once per eye, then the car / driver pass `[dll][interp]`.

**Eye distance.** `EXTRA_EYES_MARGIN_MM` (`[EXTRA_TWEAKS]`) shifts each eye sideways by half the value, sign by the
eye: the Oculus camera's `computeCameraMatrix` (`0x11c7100`, 73 B) calls the original and then multiplies a
translation of +-0.5 x value into the matrix by the sign of the eye position's X; the OpenVR camera does the same
on `GetHMDMatrixProjectionEye` (`0x11c99d0`, 72 B; eye index 1 gets -0.5, the other +0.5). Both skip when the
value is zero (global `0x182695130`) `[dll]`.

**Start camera.** `OculusManager::update` (acs.exe 0x1400a0520, 99 B) and `ViveManager::update` (0x1401d6380,
251 B) are **both replaced without calling the original** (`0x4148e0`, `0x6bb0c0`, 692 B each): at the first update
they read `vr_tweaks.ini [EXTRA_TWEAKS] KEEP_STARTING_CAMERA`; when 0 they call CSP's own
`ACCameraManager::setMode` hook (`0x107b310`) with mode 0 (cockpit), so VR starts in the driver's seat instead of
the fly-by; then they call the camera's `resetHeadPosition` (a lookup, 12-14 B in acs.exe). On every later update
they call `resetHeadPosition` again whenever two bits of a flags word (`+0xa3c` of the object behind global
`0x1826ae278`) are both set [interp: a car-reset / teleport state] `[dll]`.

**Mirror view on the monitor** (`[VR_MIRRORING]`, 17 keys): modes left eye / right eye / anaglyph (11 anaglyph
matrices) / two eyes straight / two eyes with a fake lens (aberration, darkening, distortion, blur); zoom, vertical
offset, fill-view crop, final brightness / contrast / gamma / bias, FPS counter, UI overlay mode. A stabiliser
(`[VR_MIRRORING_STABILIZER]`) reprojects the previous frame to hide head shake in the mirror. Shaders (names only;
bytecode is in embedded zip 1): `accVROutput_ps`, `accVROutput_single_ps`, `accVROutput_double_ps`,
`accVROutput_doubleStraight_ps`, `accMirrorReproject_ps`, `accMirrorReproject_carCover_ps` `[dll][ini]`.

**VR HUD** (`[VR_HUD]`, 15 keys): AC's GUI is drawn on a curved plane in the headset (arc, radius, vertical scale,
offsets, follow-camera / follow-headset), optionally after post-processing, optionally "See-Through" / "X-Ray"
when geometry covers it (`accVRXray_ps`, `accVRXray_seethrough_ps`), or, with OpenXR, as a separate cylinder
composition layer (`XR_KHR_composition_layer_cylinder`, `HUD_LAYER`). The acs.exe side is `ksgui::GUI::init3DMode`
/ `preRender3D` / `render3D` / `renderVPointer`, covered in `ui_apps.md` section 2 (rows at 0x612720, 0x611bb0,
0x6120c0) `[dll][ini]`.

**Controllers and hands.**
* Mouse pointer from a controller ray (`INTERACT_WITH_UI`), touch screens and pushable buttons in cars
  (`INTERACT_WITH_CARS`), grabbing the rear-view mirror (`INTERACT_WITH_MIRRORS`), controller markers as points and
  blue lines, haptics (`VIBRATION_INTENSITY`), driver hands follow the controllers (`CONTROLLERS_RIG`, experimental)
  `[ini]`.
* Raw state is exposed to Lua as `ac.getVR()` (`ac.StateVR`: head transform, 2 `ac.StateVRHand` with transform,
  trigger values, thumbstick, `openVRButtons` / `openVRTouches` / `openVRAxis[5]`; Oculus buttons / touches)
  `[sdk]`. "currently, accurate values are available with Oculus only" `[sdk]`.
* OpenXR input: action set "csp_hands" with pose, trigger (+touch), squeeze, thumbstick (+click, +touch),
  trackpad (+click, +touch), a / b / x / y, menu, select and a haptic output; suggested bindings for 13 interaction
  profiles (Oculus Touch, Khronos simple controller, Pico 4 / 4s / Neo 3, Meta Touch Plus, Facebook Touch Pro, HP
  Mixed Reality, HTC Vive / Cosmos / Focus 3, Valve Index, Samsung Odyssey, Microsoft motion controller) plus
  `XR_EXT_hand_tracking` (+ `data_source`, key `HAND_TRACKING`, default 0), `XR_EXT_eye_gaze_interaction` (key
  `EYE_TRACKING`, default 1; feeds VRS foveation) `[dll]` (strings `0x222a8e8`..`0x222b948`).
* Passthrough (`PASSTHROUGH` 0 / 1 steering wheel / 2 world around the car): `XR_FB_passthrough`,
  `XR_HTC_passthrough`, or an `ALPHA_BLEND` hole mask (`accOpenXRMask_ps`, `accOpenXRMask_ps_circle`) `[dll][ini]`.

**Performance tricks** `[ini]` unless marked:
* *Single Pass Stereo* (`[SINGLE_PASS_STEREO_REAL] ENABLED=1`, `USE_VS_INSTANCING=1`): both eyes in one scene
  traversal; optional single YEBIS pass for both eyes (`USE_SINGLE_POSTPROCESSING_PASS`, default 0, "disables sun
  rays"). Rendered through the replacement `renderYebis` / `renderNoPP` (`0x11c6fa0`, `0x11c7060`, `0x11c9710`,
  `0x11c9930`; each stores the camera's matrices into `0x182697a50` / `0x182697a90` and calls a CSP renderer)
  `[dll]`.
* *Variable rate shading* (`[SINGLE_PASS_STEREO] USE_VRS=1`): presets for Pimax and Quest 3, custom inner /
  middle / outer rates (`X0` = skip, `X16`..`X2` = MSAA levels, `X1`, `X1_2X1`..`X1_4X4` = shading groups) and
  areas, eye-tracked foveation, dynamic rate 7 ("experimental, currently targets 90 FPS"); "Uses NVIDIA VRWorks,
  requires 16-series NVIDIA GPU or better". Lua: `ac.setVRSConfigurationForVR`, `ac.setVRSGazeForVR`. dwrite.dll
  reaches NVAPI through `nvapi64.dll` (string `0x2086bb0`, wide `0x22b6c30`; `nvapi_QueryInterface` use at
  `0x1a5850`) `[ini][sdk][dll]`; debug overlay shaders `accVRSOverlay_ps`, `accVRSOverlay_uint_ps`.
* *Corner masking* (`[MASKING_STENCIL] ENABLED=1`): a stencil from the runtime's hidden-area / visibility mask
  (OpenVR hidden-area mesh "Hidden area mesh is available", `0x6ddcf0`, 2,172 B; OpenXR `XR_KHR_visibility_mask`,
  `0x6f0aa0`; Oculus `ovr_GetFovStencil`, `0x6dda90`) so lens-clipped corners are not shaded; `accVRCover_vs/ps`
  `[ini][dll]`.
* *Depth submission*: Oculus ASW 2.0 (`USE_ASW2`), OpenVR `SUBMIT_DEPTH` (default 0), OpenXR `SUBMIT_DEPTH`
  (default 1, `XR_KHR_composition_layer_depth`; `accCopy_depth_ps` copies / resolves it) `[ini][dll]`.
* *Frame order* (`SUBMIT_FIRST`): 0 original, 1 adjusted, 2 recommended, 3 "with hand off" (calls
  `PostPresentHandoff`) `[ini][dll]`. `SYNC_WITH_DISPLAY` adjusts the game delta-time to headset timing.
* *Other*: `RESOLUTION_MULTIPLIER` (OpenXR swap-chain size), `USE_DOWNSCALING_VR` (particles at half resolution,
  `particles_fx.ini`), `EXTRA_DITHERING`, `HUD_LAYER`, `NO_POSTPROCESSING` HUD, graphics_adjustments
  `[VR_CLIP_PLANES] NEAR_PLANE=0.05 FAR_PLANE=15000`, `AUDIO_STEREO_FIX` (see `audio.md`), crash messages always go
  through the separate reporter process in VR (`general.ini CRASH_PLUGIN` comment) `[ini]`.
* *Revive*: `REVIVE_INJECTION` 0 off / 1 SteamVR / 2 OpenXR / 3 proxy loads `LibOVRProxy64.dll`,
  `LibReviveXR64.dll`, `LibRevive64.dll`, `openvr_api64.dll` from `REVIVE_LOCATION` (`0x2297c28..0x2297c90`, in
  `0x11e82b0`) `[dll][ini]`.

**Start-up detection** (`0x11e82b0`, 10,095 B, the settings-container constructor, attach.md 1.4): reads
`vr_tweaks.ini` (`OCULUS_TWEAKS AUTO_ENABLE`, `EXTRA_TWEAKS`, `REVIVE_*`), race.ini / video.ini (`TRIPLE`,
`OCULUS`, `OPENVR`, `__EXT_OPENXR`, `FULLSCREEN`, `_EXT_FULLSCREEN`, `__CM_CUSTOM_MODE`); checks for
`OVRServer_x64.exe` and the string "oculus-runtime"; logs "Launched from Oculus runtime, switching to VR mode";
and warns "Custom rendering mode won't work. Enable post-processing and FXAA in AC video settings first." `[dll]`.

### 1.3 Triple screen

acs.exe's own mode: `CameraTripleScreen` (acs 0x1402248e0, subclass of `CameraForwardYebis`) draws the scene three
times, one `beginVirtualScreenPass(i)` + `renderPass` per screen; the in-game `TripleScreenManager` app edits the
bezel / angle / distance values stored in `Documents\Assetto Corsa\cfg\triple_screen.ini` (`[LEFT_SCREEN]`,
`[CENTER_SCREEN]`, `[RIGHT_SCREEN]` with `LEFT_POINT` / `RIGHT_POINT`, `[SPINNERS] SP_MARGINS`, `SP_ROTATION`,
`SP_DISTANCE`, `SP_WIDTH`; the owner's file exists with the defaults) `[prior]`.

CSP adds (`triple_custom.ini`, 23 settings, `[BASIC] ENABLED=1`):
* **Custom split**: `[SCREEN_SPLIT] LEFT` / `RIGHT` (default 0.333 each; 0 turns that side off = double screen),
  `FIT_SPLITS` (fit the resized splits properly). In `beginVirtualScreenPass` the split widths are
  `int(W x LEFT)`, `int(W x RIGHT)`, centre = rest; the default (1/3 each, no render scale) takes a shortcut with
  x origin = index/3 `[dll]`. `renderPass` then rescales the camera's frustum rectangle (fields +0xe80..+0xeb8 of
  the camera) by `1 - 3 x LEFT` and `1 - 3 x RIGHT` when the split is custom [interp: this is FIT_SPLITS].
* **Side screens in lower resolution** (`[FSR_OVERRIDE]`): left and right are rendered into a smaller target with
  their own scale (QUALITY 0.77 / 0.67 / 0.59 / 0.5 / 0.3333), then upscaled by FSR with `SHARPEN` and
  `ADJUST_MIP_BIAS`; needs FSR active in Graphics Adjustments. Globals: main render scale `0x1825a29d4` (shared with
  `CameraForward::onResize`, `solveBlur`, `CameraForwardYebis::initRenderTargets`), side scale `0x1825a29e4`
  `[dll][ini]`. `ac.getSim().isTripleFSRActive` `[sdk]`.
* **Camera-mode behaviour** (`[CAMERA_MODES_BEHAVIOR]`): per camera class COCKPIT, DRIVABLE, TRACK (F3), ORBIT
  (F5), CAR (F6), FREE (F7), START: 0 default, 1 force disable, 2 force enable, 3 disable (centre part only),
  4 enable (centre part only). "Centre only" sets a flag (`0x18268b23d`) that makes `beginVirtualScreenPass` skip
  screens 0 and 2 `[dll][interp]`. The caller of this key sits in the camera managers (`misc_core` group), not read.
* **Custom cameras per screen** (the commented `[CAMERA_LEFT]`, `[CAMERA_CENTER]`, `[CAMERA_RIGHT]` blocks: `MODE`
  = `CAR` / `CAMERA` / `ORIGINAL`, `POSITION`, `LOOK`, `UP`, `FOV`, `ASPECT_MULT`): when present, a controller
  object (`0x1826ab110`, a three-method interface: render override, viewport, camera apply) takes over both
  hooked methods and the original is not called `[dll][ini]`.
* **Vertical margins** (`[VERTICAL_MARGINS] LEFT/CENTER/RIGHT = top, bottom`): before the original pass each
  screen gets full-width quads of that height at top and bottom (a CSP constant buffer + a fixed state); "Margins
  offset UI, help to avoid losing apps and save a bit on rendering performance" `[dll][ini]`.
* **Names and UI**: `ALWAYS_SHOW_DRIVER_NAMES`, `CUSTOM_DRIVER_NAMES_PROJECTION2` (reader `0xe8b0a0`, 4,455 B);
  `splashscreen.ini TRIPLE_DETAILS_IN_CENTER=1`; `extra_fx.ini ALLOW_IN_TRIPLE=0`; `car_instruments.ini CENTER_X`
  uses `screen.triple` in its Lua expression; AC's own app gets the new split live
  (`TripleScreenManager::onSpinnerValueChanged` hook, then CSP fires `ac.onTripleConfigurationChanged`) `[ini][dll]`.
* **Lua**: `ac.getTripleConfiguration()` (3 screens with `left`, `right`, `height`, `xFrom`, `xTo`, `xWidth`),
  `ac.isTripleModeActive()`, `ac.getTripleScreenViewport(i)`, `ac.setTripleScreenSideValues(i, left, right,
  height)` (live, not saved), `ac.onTripleConfigurationChanged`; `render.ProjectFace.Auto` (current triple face
  in the main pass) and the shipped `lua/pp-filters/curved` filter use them `[sdk][lua]`.
* Post-processing variants: `accTAA_triple_ps`, `accTAA_triple_old_ps`, `accTAA_tripleExtend_ps`,
  `accMotionBlur_ps_triplefsr{,_low,_lowest}`, `accMotionBlur_fillbg_triple_ps` (see `rendering_core.md`);
  `YebisPP::applyPostProcessing` (0x11cdc50) is the shared VR / triple wrapper `[dll]`.

### 1.4 Custom render modes (`custom_rendering_modes.ini`)

Module "Display mode: custom" (`FULLNAME`), loaded by `0xf95550` (1,852 B; also reads graphics_adjustments,
general, freer_camera and walking_out ini files). Log on this PC: "Extension `Custom Rendering Modes` disabled:
custom mode is not selected". Needs post-processing **and** FXAA on (hint line + start-up warning) `[ini][dll]`.
* `[MODE_360]` scene drawn six times into `FRAME_RESOLUTION` (256..3072, default 1024) square sub-frames with
  `FRAMES_MARGIN` (0.1) overlap, then reprojected by `accMRT360_ps`. `[MODE_FISHEYE]` the same with five frames
  and `accMRTFisheye_ps`; `MODE` 0 stereographic, 1 equidistant (default), 2 equisolid, 3 orthographic;
  `SCALE_LINEAR` 0.6, `SCALE_ANGLE` 1. Post pass `0xc6e130` (1,517 B; profiler labels "Generate MIPs",
  "Reprojection") `[ini][dll]`.
* `[MODE_SPLITSCREEN]` (`HORIZONTAL_LAYOUT`, `TWO_RAW_MICE`; not in the shipped ini, written live by the new mode's
  Lua with `ac.applyLiveConfigEdit`): two cars in one world on two viewports. "Main part is done on C++ side"
  (the Lua file's own comment) `[lua]`. The Lua part (`lua/new-modes/splitscreen/mode.lua`, 2,779 B) only loads the
  second car's controls preset (`ac.loadCarControlsConfiguration(1, ...)`, default `controllers/presets/X360Default.ini`),
  syncs ABS / TC / auto-shifter / wastegate between the cars via `ac.setTargetCar`, switches both cars' headlights,
  hides the HUD, disables the quick pit menu and optionally runs traffic. The mode manifest sets
  `RENDER_MODE=__EXT_SPLITSCREEN` in `[TWEAKS]`, `IMMEDIATE_START=1`, rules BASE_MODE=RACE, AI_LIMIT=1
  `[lua][ini]`. The C++ side is not located (section 8). Also: `ac.CSPModuleID`, Lua `ac.getSim().isCustomVideoMode`
  "(fisheye, 360°, splitscreen)" `[sdk]`.
* `custom.comparison` and `[BASIC] DEBUG_MODE` (the debug key exists in the ini): see section 8.

### 1.5 Window, DPI, swap chain, V-sync, VRR (`dxgi_tweaks.ini`, module "Other: DXGI")

Order of events (all `[dll]` strings / log lines unless marked):

1. **DPI awareness** (function `0xc73560`, 4,458 B = decomp_infra; log "Setting DPI-aware flag: ..., final state:
   per-monitor-aware v1"). Key `[COMMON] DPI_AWARE3` (default 2, the key is commented in the ini): 0 do nothing
   (logs "Not setting DPI-aware flag, currently: ..."), 1 `SetProcessDPIAware`, 2 try
   `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` (access denied = "already set before us"), then
   `SetProcessDpiAwareness(2)`, then `SetProcessDPIAware`. Runs **even when the module is disabled**
   (decompiled branch). Helper `0xc73380` prints the state names (unaware / system-aware / per-monitor v1 / v2).
2. **Module gate** (same function): the module turns itself off with "not available with MSAA without
   post-processing" when video.ini `[VIDEO] AASAMPLES` > 0 and post-processing is off (this PC: AASAMPLES=4,
   `[POST_PROCESS] ENABLED=0`). Then every swap-chain tweak below is off and the globals are zeroed `[dll][log-csp]`.
3. Reads (in the same function): `[BASIC] MAXIMUM_LATENCY_BASE`, `ADVANCED_PACING`, `MAXIMUM_LATENCY_ADVANCED`;
   `[COMMON] BACKGROUND_PAUSE`, `EFFICIENCY_MODE`; `[WINDOW_TWEAKS] BORDERLESS_WINDOWED`; `[COMPATIBILITY]
   EXCLUSIVE_FULLSCREEN` (via `0xc71990`, 4,190 B); `CONFINE_CURSOR`, `SET_TOPMOST`, `FORCE_FOCUSED` (via
   `0xc72f70`, 317 B). In VR the latency / pacing / background-pause values are forced off (the gate byte at
   `settings+0x1c` [interp]; ini hint: "Settings related to latency, pacing and VRR have no effect in VR modes").
4. **Monitors and placement** (log "Detected monitors: Main: [pos=0, 0, size=1920, 1080, dpi=96] / #0: [pos=-3072,
   0, size=3072, 1920, dpi=192] / #1: ..." from `0x3f5ed0`, 796 B, with `GetDpiForMonitor` `0x3f61f0`). The
   placement functions are `0x3f30b0` (1,961 B, saved rectangle: "Placing AC window (rect: [...], requested size:
   W, H, monitor encompasses | overlaps window, rect is good to go)", "Window is outside: ..., resetting to the
   middle of ...") and `0x3f3860` (2,147 B, from scratch: "Size is close to the entire desktop area, window will
   cover everything", "Size exceeds the size of the monitor, computing the whole desktop area", "Size is close to
   the monitor size (...), total number of monitors: N", "Size is smaller than the monitor size (...), centering
   within, scaled size: ...", "Size is within the size of the monitor, using optimal window size for the
   monitor"). The decision is cached in **video.ini `[VIDEO] _EXT_PLACEMENT`** as
   `px%d_py%d_w%d_h%d_r%d_i%d_f%d_e%d,<x>,<y>,<w>,<h>,<reqW>,<reqH>` (format string `0x21fc0c8` in `0x3eea40`):
   the owner's file has `px0_py0_w1920_h1080_r60_i0_f0_e0,160,160,2752,1600,1920,1080`, which is exactly the
   rectangle in the log line. `SAVE_WINDOW=1` additionally keeps `cfg\extension\window_position.ini
   [AC_WINDOW_POSITION] REAL_X REAL_Y X Y` (owner's: 0, 0, 8, 31). With VR on, `vr_tweaks.ini [EXTRA_TWEAKS]
   ENSURE_AC_IS_NOT_CROPPED` is read in `0x3f30b0` [interp: keeps the window fully on a monitor so VR mouse
   movement works]. Window creation: `0x3f40d0` (4,955 B; titles "%ls (Previews)", "(Showroom)", "(Headless)";
   `__EXT_TOGGLE_FULLSCREEN`; "Failed to create a window"), `0x3f13b0` (6,092 B; "Initializing DirectX",
   "DirectX initialization is taking too long?" with the advice list: restart Steam, restart PC, remove `d3d11.dll`
   / `dxgi.dll` from the AC folder, update drivers, disable overlays), `0x3f2ca0` (icon `ac.ico` from the dll),
   `0x3f77c0` (`DwmExtendFrameIntoClientArea` for the shadowed borderless style), `0x3f7996`
   (`AdjustWindowRectExForDpi`), `0x3f5ac0` ("Using desktop resolution.").
5. **Fullscreen model**: by default "fullscreen" is a **borderless window covering the monitor**
   ("borderless, covering the whole screen" vs "exclusive", Render Stats app); `EXCLUSIVE_FULLSCREEN=1` allows
   `SetFullscreenState` (function `0x3f54c0`). Alt+Enter = `__EXT_TOGGLE_FULLSCREEN` (`ac.toggleFullscreen`).
   `BORDERLESS_WINDOWED` 0 / 1 with shadow / 2 flat / 3 flat and immovable; `SET_TOPMOST`, `FORCE_FOCUSED`,
   `CONFINE_CURSOR` take 0 / 1 / 2 only in fullscreen / 4 only in VR / 3 fullscreen or VR `[ini]`.
6. **Device and swap chain** (`createDeviceAndSwapChain` replacement `0x6c3f50`, ~5 KB, decompiled in
   `decomp_req1`): `CreateDXGIFactory1`, adapter chosen by `0x6c10d0` (5,240 B; `SELECT_ADAPTER` 0 automatic /
   1 selected monitor / `NVIDIA?` `AMD?` `Intel?`; the OpenXR runtime's adapter LUID wins: "Using OpenXR D3D11
   adapter", "OpenXR adapter LUID was not found among DXGI adapters"; log "Looking for the target GPU: 8.58 ms"
   and "Target GPU: %s (VID: 0x%x)"), `D3D11CreateDevice` (debug flag only with the debug switch), thread priority
   from `general.ini DX_DEVICE_PRIORITY` (7), then a **DXGI flip-model swap chain** through
   `IDXGIFactory2::CreateSwapChainForHwnd` ("Initializing DirectX with DXGI flip model", then "DirectX swapchain
   created: flip=, VRR=, tearing="); format R8G8B8A8_UNORM, or R16G16B16A16_FLOAT with `[HDR] ENABLED`; buffer
   count 2 (+1 with HDR or tearing, from the decompile); `MakeWindowAssociation(NO_WINDOW_CHANGES |
   NO_ALT_ENTER)` unless `OLD_WINDOW`; `SetMaximumFrameLatency` on the DXGI device (`MAXIMUM_LATENCY_BASE`) or on
   the swap chain with a waitable object (`ADVANCED_PACING`, `MAXIMUM_LATENCY_ADVANCED`, string "To ensure lowest
   latency, CSP waits for GPU to signal the optimal time to start drawing the next frame", label "Waiting for a
   hint from GPU"). HDR: `CheckColorSpaceSupport` / `SetColorSpace1(G2084 P2020)`; fallback through
   `QueryDisplayConfig` + `DisplayConfigGetDeviceInfo` (advanced colour); `[HDR] VERIFY=0` skips the check
   ("HDR is not available. Disable HDR in DXGI settings to slightly speed up loading."). `D3D11CreateDeviceAndSwapChain`
   itself is hooked (`0x6c5170`: "No redirect for this device", "Pre-initializing MMF: creating offscreen DirectX
   device"). acs.exe's globals `device`, `immediateContext`, `swapChain`, `videoSettings` are filled with CSP's
   objects through data-symbol lookups `[dll]`.
7. **VRR / V-sync** (`0x6c2760`, 950 B: "Checking if VRR should be enabled": reads AMD FreeSync through ADLX
   `amdadlx64.dll`, the NVIDIA driver profile (V-sync / G-sync / "VRR blocked") through NVAPI, and Windows'
   `Software\Microsoft\DirectX\UserGpuPreferences\DirectXUserGlobalSettings` values `AutoHDREnable`,
   `SwapEffectUpgradeEnable`, `VRROptimizeEnable` in `0x6c39a0`, 766 B). `ALLOW_VRR` 0 cap / 1 auto / 2 uncapped
   with `DXGI_PRESENT_ALLOW_TEARING` (default 2). `VSYNC_MODE` 0 default `Present(1)` / 1 "FPS limiter logic" /
   2 wait for v-blank; `VSYNC_ADAPTIVE`; `FORCED_VSYNC_AWARE` (when the GPU driver forces V-sync CSP detects
   frames coming out in whole refresh periods and waits for the v-blank itself at the start of the frame:
   `0x6c62e0`, `0x6c64c0`, 1,263 B). The present function is `0x6c69b0` (2,386 B; window title switches to
   "Assetto Corsa (occluded)" with `CONSIDER_OCCLUDED_FLAG`, the pause when the system is asleep). Resize:
   `kglResizeBuffers` replacement `0x117dcb0`; "Resolutions match, direct output" vs "Resolutions do not match,
   proxied output" (log on this PC at 9.57 s; `0x117d670`) with `accCopyResolve_msaa%d_ps` and `OLD_STRETCH`
   `[dll][log-csp]`.
8. **FPS limits**: AC's `[VIDEO] FPS_CAP_MS` (owner's 0), the borderless window's refresh rate, a limiter set by
   a CSP module or script (`ac.applyRenderingLimits({fpsLimit=...})`, `ac.disableFrameLimiter`), `BACKGROUND_PAUSE`
   (2 FPS / 30 FPS / pause; `EFFICIENCY_MODE` uses the Windows efficiency API while idle), and
   `general.ini [THREAD_TWEAKS]` (`RENDER_TIMING=4` high-resolution timer, `PHYSICS_TIMING=4`, `PROCESS_PRIORITY=1`,
   MMCSS) `[ini][sdk]`. The Render Stats app prints the swap chain flags (NONPREROTATED, ALLOW_MODE_SWITCH,
   GDI_COMPATIBLE, FRAME_LATENCY_WAITABLE_OBJECT, ALLOW_TEARING, FOREGROUND_LAYER, FULLSCREEN_VIDEO, YUV_VIDEO,
   HW_PROTECTED), "V-sync state", "VRR state", "Max frame latency", "Fullscreen: exclusive | borderless" and the
   frame-limiter source: a text oracle for these (section 7).

### 1.6 On this PC

`custom_shaders_patch.log` (2026.10.15): DXGI Tweaks disabled (MSAA without post-processing), DPI flag "already set
before us", `Importing DLL: openvr_api.dll` (the delay-load hook of attach.md 1.7, no VR started), 3 monitors found,
window placed from the saved rectangle, "Resolutions do not match, proxied output", GPU = AMD Radeon Pro 5500M on an
Apple MacBookPro16,1 running Windows 10 with an external 1920x1080 @ 59.94 Hz monitor as primary. No `vr_tweaks.ini`,
`triple_custom.ini`, `dxgi_tweaks.ini` or `custom_rendering_modes.ini` exists in `Documents\Assetto Corsa\cfg\
extension` (all defaults) `[log-csp]`.

## 2. Game parts it replaces or extends

### 2.1 Stereo cameras, managers and the OpenVR settings accessor (29 rows of group `vr_screens`: 23 hooks + 6
lookups; plus `OculusTexture::Init` from the rendering group). The 3 `CameraTripleScreen` rows (the 32nd, 31st and
30th of the group) are in 2.2.

Slots are `.data` RVAs holding the trampoline. "acs" columns: VA / size.

| Symbol | acs.exe VA / size | CSP RVA / size | Slot | Cat | What it changes |
|---|---|---|---|---|---|
| `OculusManager::update` | 0x1400a0520 / 99 | 0x4148e0 / 692 | 0x2699848 | C | start camera and head reset, original never called (1.2) |
| `ViveManager::update` | 0x1401d6380 / 251 | 0x6bb0c0 / 692 | 0x26a8598 | C | same for OpenVR |
| `StereoCameraForward::StereoCameraForward` | 0x140211370 / 3639 | 0x11c6650 / 48 | 0x268c950 | A | original, then CSP post-construct (0x11c8520) |
| `...::renderMirrorTexture` | 0x1402137b0 / 485 | 0x11c69c0 / 903 | 0x268c940 | B | with CSP's mirror target: copies eye textures into it, else original inside a profiler scope (and `0x181342e80` closure that zeroes a mip bias when `0x18268ca54` != 0) |
| `...::normalRendering` | 0x140213030 / 503 | 0x11c6d80 / 21 entry + body | 0x268c948 | C | full replacement, SPS / VS instancing (1.2) |
| `...::oculusRenderPass` | 0x140213230 / 909 | 0x11c6da0 / 510 | 0x268c990 | B | shifts the camera by the extra-IPD globals `0x1825e2f08..10`, fires CSP pre / post pass event lists, original per eye |
| `...::renderYebis` | 0x140213b10 / 1072 | 0x11c6fa0 / 177 | 0x268c998 | C | stores matrices, calls CSP's YEBIS-for-VR renderer (0x180eaa4f0) |
| `...::renderNoPP` | 0x1402139a0 / 365 | 0x11c7060 / 158 | 0x268c980 | C | same without post-processing (0x180ea7790) |
| `...::computeCameraMatrix` | 0x140212900 / 550 | 0x11c7100 / 73 | 0x26ade30 | B | original + `EXTRA_EYES_MARGIN_MM` |
| `...::computeEyePoses` | 0x140212b30 / 470 | - | 0x268c988 | D | called by CSP |
| `...::resetHeadPosition` | 0x140213f40 / 12 | - | 0x268c968 | D | called by the Manager::update replacements |
| `...::finishRendering` | 0x140212d10 / 496 | 0x11c8180 / 227 | 0x26ade58 | C | CSP submit `0x11c7980`; swaps two rect fields when render scale != 1 |
| `...::initOculus` | 0x140212fd0 / 84 | 0x11c8270 / 40 | 0x26ade60 | B | original; on failure formats the error text and shows it (`0x11c82b0`) |
| `OculusTexture::Init` (rendering_hooks) | 0x140212310 / 607 | 0x11c6010 / 230 | 0x26ade08 | B | original inside a profiler scope when SPS is not active |
| `StereoCameraVive::StereoCameraVive` | 0x140220b80 / 3232 | 0x11c8f90 / 48 | 0x268c9e0 | A | as the Oculus constructor |
| `...::renderMirrorTexture` | 0x140223910 / 398 | 0x11c9300 / 539 | 0x268c9d8 | B | as Oculus |
| `...::normalRendering` | 0x140223520 / 503 | 0x11c9520 / 21 entry + body | 0x268c9c8 | C | as Oculus (field +0x7a8 instead of +0x7b0) |
| `...::viveRenderPass` | 0x140224620 / 695 | 0x11c9540 / 462 | 0x268c9d0 | B | as `oculusRenderPass` |
| `...::renderYebis` | 0x140223f60 / 914 | 0x11c9710 / 533 | 0x268c9b8 | C | profiler label "Bind shadows"; CSP renderer 0x180eaaaf0 |
| `...::renderNoPP` | 0x140223aa0 / 1202 | 0x11c9930 / 158 | 0x268c9c0 | C | CSP renderer 0x180ea7e50 |
| `...::GetHMDMatrixProjectionEye` | 0x140221ac0 / 307 | 0x11c99d0 / 72 | 0x26ade70 | B | original + extra eye distance |
| `...::computeCameraMatrix` | 0x140222850 / 593 | - | 0x268c9f0 | D | lookup |
| `...::computeEyePoses` | 0x140222ab0 / 366 | - | 0x268c9f8 | D | called by `render` |
| `...::resetHeadPosition` | 0x140224300 / 14 | - | 0x268c9e8 | D | lookup |
| `...::initOpenVR` | 0x140223440 / 223 | 0x11c9c90 / 714 | 0x26ade80 | B | original called from a closure (0x11cbfa0 -> slot) in a guarded scope; labels "Initializing OpenXR" / "Initializing OpenVR"; failure: "Failed to initialize VR." + SteamVR hint dialog |
| `vr::VRSettings` | 0x140222730 / 140 | 0x11ca4d0 / 163 | 0x26ade88 | B | returns a proxy object (vtable 0x182294a58) when OpenComposite is detected |
| `vr::VRCompositor` | 0x1402226a0 / 140 | - | 0x268c9b0 | D | lookup |
| `...::UpdateHMDMatrixPose` | 0x140221c00 / 2706 | 0x11ca710 / 930 | 0x26ade90 | B | original in scopes "Tracking space fix" (OpenComposite), "VR synchronization", then "Events polling" (quit events) |
| `...::render` | 0x140223720 / 482 | 0x11cb110 / 839 | 0x26adea8 | C | whole frame (1.2) |
| `...::finishRendering` | 0x140223210 / 266 | 0x11cbaa0 / 963 | 0x26adeb0 | C | `IVRCompositor::Submit` x2 (flag 0x10 = with depth, per-eye matrix + range 0..1), `PostPresentHandoff` in mode 3, flush before or after by `SUBMIT_FIRST` |

Totals of the whole group (these rows plus the 3 `CameraTripleScreen` rows of 2.2): 26 hooks (24 measured
functions = 14,515 B + the two `normalRendering` chunks, 8.6 KB pseudo-C each), 6 lookups, 6 classes
(`OculusManager`, `ViveManager`, `CameraTripleScreen`, `StereoCameraForward`, `StereoCameraVive`, `vr`) `[dll]`.
Category mix of the 26 hooks: A 3 (the three constructors), B 12, C 11; D 6 (the lookups).

### 2.2 Triple screen and camera form

| Symbol | acs.exe VA / size | CSP RVA / size | Slot | Cat | What it changes |
|---|---|---|---|---|---|
| `CameraTripleScreen::CameraTripleScreen` | 0x1402248e0 / 692 | 0x10a7990 / 39 | 0x268bca0 | A | original via helper 0x10a79c0, then registers the object (0x10aa760) |
| `CameraTripleScreen::beginVirtualScreenPass` | 0x140224e30 / 848 | 0x10a8c70 / 2127 | 0x268bc98 | B (C with custom cameras) | split widths, side render scale, per-screen rectangles, "middle only"; original except when the custom controller exists |
| `CameraTripleScreen::renderPass` | 0x140225180 / 2349 | 0x10a99a0 / 3408 | 0x268bc90 | B (C with custom cameras) | margins, FIT_SPLITS rescale, original last |
| `TripleScreenManager::TripleScreenManager` | 0x1402ff920 / 3563 | 0x6a7f70 | 0x26a7540 | A | stores `this` (global 0x1826a7538), tail-jumps |
| `TripleScreenManager::~TripleScreenManager` | 0x140300710 / 1581 | 0x6a7f90 | 0x26a7548 | C | CSP re-implementation that writes `triple_screen.ini` (15,722 B pseudo-C; keys listed in `ui_apps.md` 2.4) |
| `TripleScreenManager::onSpinnerValueChanged` | 0x140301b90 / 834 | 0x6a7fa0 / 187 | 0x26a7550 | B | original, then walks a list of listeners and schedules a delayed call (15.0 s timer key `0xfb90d4d8b5cb0a55`) [interp: the Lua `ac.onTripleConfigurationChanged` event] |
| `FormCamera::FormCamera` | 0x1402f1bc0 / 15547 | 0x5dbb90 / 51 | 0x268b0f8 | A | singleton capture (`0x182690be8`), then original and a CSP init (0x5dbbd0) |

### 2.3 Attachment points outside the `Class::method` table (replacement RVAs read from the registration code)

The six OpenVR exports and the window / device entry points are registered by name without a class; their record
is (name pointer, replacement, slot). I decoded the registration functions; RVAs below are **verified** `[dll]`.

| Name | Replacement RVA (size) | Slot | Notes |
|---|---|---|---|
| `\openvr_api\VR_InitInternal` | 0x6f3060 (326) | 0x26a9198 | "OpenXR failed to initialize: ..." |
| `\openvr_api\VR_GetVRInitErrorAsEnglishDescription` | 0x6f31b0 | 0x26a91a0 | |
| `\openvr_api\VR_ShutdownInternal` | 0x6f31d0 (18) | 0x26a91a8 | |
| `\openvr_api\VR_IsInterfaceVersionValid` | 0x6f3230 (146) | 0x26a91b0 | compares the `IVRSystem_` / `IVRSettings_` / `IVRCompositor_` prefixes |
| `\openvr_api\VR_GetGenericInterface` | 0x6f32d0 (149) | 0x26a91b8 | returns CSP's OpenXR-backed objects |
| `\openvr_api\VR_GetInitToken` | 0x6f3370 (41) | 0x26a91c0 | |
| `ovr_GetTrackingState` | 0x6de950 (507) | 0x26a9188 | the other `ovr_*` names are lookups |
| data `pHMD` (`IVRSystem*`) | lookup | 0x26ade68 | event polling |
| data `hLibOVR` | lookup | 0x26ade10 | |
| `\USER32\MessageBoxA` (twice) | 0x11cad90 (800), 0x11cb0b0 (95) | 0x26ade98, 0x26adea0 | next to the stereo registrations: VR error dialogs go to `plugins/AcTools.ErrorReporter.exe` [interp] |
| `kglOpenWindow` | 0x3f5690 | 0x2699828 | window creation |
| `RenderWindow::RenderWindow` | 0x3f5ac0 (868) | 0x2699810 | "Using desktop resolution." |
| `\USER32\RegisterClassW` | 0x3f2c60 | 0x2699838 | AC's window class |
| `\USER32\GetWindowRect` | 0x3ee180 | 0x26997f8 | |
| `WndProc` | lookup | 0x2699840 | |
| `initDX11` | 0x107ce00 (615) | 0x26aa900 | label "Initializing DirectX" |
| `createDeviceAndSwapChain` | 0x6c3f50 | 0x26a8620 | 1.5 step 6 |
| `\D3D11\D3D11CreateDeviceAndSwapChain` | 0x6c5170 | 0x26a8628 | |
| `kglInit` | 0x6c5d00 | 0x26a8638 | |
| `kglResizeBuffers` / `kglSwapBuffers` | 0x117dcb0 / 0x117caa0 | 0x26ad570 / 0x26ad510 | |
| `\USER32\GetCursorPos`, `SetCursorPos`, `ShowCursor`, `SetCursor` | 0xb70500, 0xf4b220, 0xf4b290, 0x305980 (3-byte stub) | 0x26a9860, 0x26aa6e8, 0x26aa6f0, 0x26aa6e0 | cursor handling shared with input (`input_ffb.md`) |
| `checkRaceINI` | 0x3558e0 | 0x26a8f50 | |
| `\KERNEL32\CreateFileA / W` | 0x6cae80 / 0x6cb0f0 | 0x26a8ed0 / 0x26a8ed8 | file redirection (attach.md 1.8) |
| `__HrLoadAllImportsForDll` | 0x6ccc90 | 0x1826a8f00 | logs "Importing DLL: openvr_api.dll" and resolves the `\openvr_api\` names once it is loaded (attach.md 1.7) |

Not hooked (acs.exe keeps its own code): `StereoCameraVive::configureOpenVR` (reads `cfg/openvr.ini`),
`setRenderMultiplier`, `setEyesDistance`, `GetHMDMatrixPoseEye`, `FormOpenVR` (the in-game OpenVR app), the
LibOVR shim itself, `OculusManager::OculusManager` (reads `cfg/oculus.ini [SETTINGS]
AUTOSELECT_RIFT_AUDIO_DEVICE`). Rows of other briefs that belong to VR / screens: `ksgui::GUI::init3DMode /
preRender3D / render3D / renderVPointer` (`ui_apps.md`), `TrackIR::*` (3 hooks) and `MouseHider::*` (4 hooks)
(`input_ffb.md`), `ScreenCapturer::*` (4 hooks; `screenshot_vr_left.jpg` / `_right.jpg`, Steam
`addVRScreenshotToLibrary`, `Stereo`, `StereoPanorama`), `VirtualMirrorRenderer::*`, `YebisPP::*` (VR / triple
post-processing wrappers), `ACCameraManager::setMode / update`, `CameraOnBoard::*`.

## 3. Config keys and Lua API

### 3.1 `extension/config/vr_tweaks.ini` (module "Display mode: VR")

163 key lines in 21 sections = 5 header + 3 `__HINT_BOTTOM` hints + 1 duplicate (`VRS_GAZE_FOV` appears twice) + 154
settings; 21 are marked hidden. Names and defaults from the shipped file `[ini]`; reader `0xe94a80` (14,042 B)
`[dll]`.

| Section | Keys (default) |
|---|---|
| `[BASIC]` | `ENABLED=1` (no-op, hidden), `SYNC_WITH_DISPLAY=0`; `ENABLED2` is a commented-out kill switch ("not supported and will break things") |
| `[SINGLE_PASS_STEREO_REAL]` | `ENABLED=1`, `USE_VS_INSTANCING=1` (hidden), `USE_SINGLE_POSTPROCESSING_PASS=0` |
| `[SINGLE_PASS_STEREO]` (VRS) | `USE_VRS=1`, `VRS_PRESET=CUSTOM`, `VRS_RATE=3`, `VRS_AREA=2`, `VRS_GAZE_ALIGN_EYES=1`, `VRS_GAZE_DEBUG=0`, `CUSTOM_VRS_RATE_INNER=X2`, `..._MIDDLE=X1`, `..._OUTER=X1_4X4`, `CUSTOM_VRS_AREA_INNER=0.3, 0.3`, `..._MIDDLE=0.5, 0.5`, `..._OUTER=0.7, 0.7` (ignored), `VRS_SUPERSAMPLING=0`; hidden: `VRS_GAZE_SHIFT_L/R`, `VRS_GAZE_FOV`, `VRS_GAZE_EYE_L/R`, `VRS_GAZE_EYE_TRACKING=1`, `VRS_GAZE_FLIP_Y=1` |
| `[VRS_PRESET_*]` x6 (hidden) | `RATE_OUTER/MIDDLE/INNER`, `AREA_OUTER/MIDDLE/INNER`. PIMAX_RECOMMENDED X1_4X4 / X1 / X2, areas 1.5,1.5 / 1.5,0.5 / 1.1,0.3; PIMAX_LOW X1_4X4 / X1_2X1 / X1, 1.5,1.5 / 1.5,0.5 / 1.5,0.3; PIMAX_HIGH X1_4X4 / X2 / X2, 1.5,1.5 / 1.5,0.6 / 1.5,0.4; QUEST3 X1_4X4 / X1 / X2, 1.5,1.5 / 1.2,1.0 / 0.85,0.65; QUEST3_LOW X1_4X4 / X1_2X1 / X1 and QUEST3_HIGH X1_4X4 / X2 / X2 with the Quest 3 areas. `VRS_RATE` 1 highest performance (no MSAA), 2 high performance (no MSAA), 3 balanced (MSAA 4x), 4 high quality (4x), 5 highest quality (8x), 6 custom, 7 dynamic |
| `[MASKING_STENCIL]` | `ENABLED=1`, `NORMALIZE_STENCIL=1`, `FLIP_STENCIL=0` |
| `[OCULUS_TWEAKS]` | `USE_ASW2=1`, `EXIT_AWARE=1`, `UPDATE_MANIFEST=1`, `UPDATE_MANIFEST_RACE_DETAILS=1`, `AUTO_ENABLE=1` |
| `[OPENVR_TWEAKS]` | `SUBMIT_FIRST=1`, `SUBMIT_DEPTH=0`, `OPENCOMPOSITE_COMPATIBLE=-1` (auto / 0 no / 1 original / 2 ACC version), `OPENCOMPOSITE_HANDLE_CRASHES=1`, `POINTING_ANGLE=30` (hidden), `AXIS_THUMBSTICK=1`, `AXIS_INDEX=2`, `AXIS_HAND=3`, `LOADING_DETAILS=1` |
| `[OPENXR_TWEAKS]` | `RESOLUTION_MULTIPLIER=1` (0.25..3), `EYE_TRACKING=1`, `HUD_LAYER=0`, `HAND_TRACKING=0`, `SUBMIT_DEPTH=1`, `PASSTHROUGH=0`, `PASSTHROUGH_STEERING_WHEEL_RADIUS=0.2`; hidden: `COLOR_SPACE=SRGB`, `TRACKING_SPACE=AUTO`, `BLEND_MODE=OPAQUE`, `RUNTIME_JSON=`, `NO_XR_SWAPCHAIN_USAGE_TRANSFER_DST_BIT=0` |
| `[CONTROLLERS_INTEGRATION]` | `INTERACT_WITH_UI=1`, `INTERACT_WITH_CARS=1`, `INTERACT_WITH_MIRRORS=1`, `SHOW_CONTROLLERS_POINTS=1`, `SHOW_CONTROLLERS_LINE=1`, `VIBRATION_INTENSITY=1`, `CONTROLLERS_RIG=0`; hidden `SHIFT_POSITION=0,0,0`, `SHIFT_ROTATION=0,0,0` |
| `[MOUSE_POINTER]` | `COLOR=1,1,0`, `CLICKED_COLOR=1,0.5,0`, `FASTER_FADING2=1`, `LOWER_REFRESH_RATE_FOR_MOUSELESS=0` (not allowed online) |
| `[RAYCAST]` | `RAYCAST_EYE=0` (left; 1 right; -1 middle) for Objects Inspector rays |
| `[VR_MIRRORING]` | `ENABLED=1`, `SEPARATE_EYES=0` (0 left, 3 right, 2 anaglyph, 4 two straight, 1 two distorted), `ZOOM=0.98`, `VERTICAL_OFFSET=0`, `FILL_VIEW=1`, `ANAGLYPH_MODE=RED_CYAN` (11 modes), `LENS_ABERRATION=1`, `LENS_DARKENING=1`, `LENS_DISTORTION=0.2`, `BLUR_BRIGHTNESS=0.8`, `BLUR_STRENGTH=2.5`, `PP_BRIGHTNESS=1`, `PP_CONTRAST=1`, `PP_GAMMA=1`, `PP_BIAS=0`, `SHOW_FPS_COUNTER=0`, `SHOW_UI=0` (0 no, 1 yes, 2 yes and hide in headset, 3 nowhere) |
| `[VR_MIRRORING_STABILIZER]` | `ENABLED=0`, `ZOOM=0.9`, `FADE=1`, `SPEED=0.5`, `DRAG=0.5` |
| `[VR_HUD]` | `ENABLED=1`, `NO_POSTPROCESSING=1`, `NO_MIRROR=0`, `ONLY_IN_FIRST_PERSON=0`, `MOVE_WITH_FIRST_PERSON_CAMERA=1`, `MOVE_WITH_CAMERA=1`, `MOVE_WITH_HEADSET=0` (0 / position / full), `SHAPE_ARC=140`, `SHAPE_VERTICAL_SCALE=1.0`, `SHAPE_RADIUS=0.4`, `BRIGHTNESS_MULT=1`, `BASE_OFFSET=0,0,0`, `CAMERA_OFFSET=0,0,0`, `RENDER_MODE=0` (original / see-through / x-ray), `RENDER_OCCLUDED_OPACITY=0.2` |
| `[EXTRA_TWEAKS]` | `ENSURE_AC_IS_NOT_CROPPED=1`, `MOVEMENT_AWARE=1` (moving headset: walking away from the car switches audio to exterior), `ALLOW_TO_CHANGE_FILTERS=0`, `LOAD_ACTUAL_FILTER=0`, `EXTRA_EYES_MARGIN_MM=0`, `EXTRA_DITHERING=0`, `AUDIO_STEREO_FIX=0` (0 / only in VR / always), `HUD_REPLAY_TWEAK=1`, `KEEP_STARTING_CAMERA=0`, `STARTING_CAMERA_HEIGHT=1.65` (hidden), `HORIZON_LOCK_OUTSIDE=0`, `REVIVE_INJECTION=0`, `REVIVE_LOCATION=C:/Program Files/Revive` |

Other VR / screen keys in other files `[ini]`: `graphics_adjustments.ini [VR_CLIP_PLANES] NEAR_PLANE=0.05
FAR_PLANE=15000`; `small_tweaks.ini HIDE_DRIVER_IN_CARS_WITH_HSHIFTER_IN_VR=0`, `HIDE_DRIVER_SUIT=0` (0 / in VR / always),
`SHOW_DRIVER_AND_HANDS_IN_VR=0`; `smart_mirror.ini VR_STEREO=0`, `VR_LOCK=0`, `[SIMPLE_TILTING_VR]` (hidden);
`neck.ini ENABLED_IN_VR=1`, `[ALIGNMENT_VR]`, `[GFORCES_VR]`, `[EFFECTS_VR]` (`LENS_FLARE=0`); `particles_fx.ini
USE_DOWNSCALING_VR=0`; `smart_shadows.ini LOOKLESS=2` (not in VR); `gui.ini VR_MODE=0` (hidden); `nice_screenshots.ini
ENABLED_IN_VR=0` (hidden); `general.ini` comment `VR_FIX=1` (commented, hidden: stereo sound); `data_controls_props.ini
__CM_RESET_CAMERA_VR` "Reset camera in VR"; `track_adjustments.ini SNAP_TYRE_GROOVES_TO_SURFACE` ("not available in
VR"); `car_instruments.ini USE_NEW_WIPERS2` (comment "might malfunction in VR"); `extra_fx.ini` description "For now,
only works without VR". Per-car / per-track configs: only a comment in `tracks/loaded/ks_nordschleife.ini` (glass
windows in VR); the owner's 204 cars and 26 tracks add no VR / triple key `[ini]`.

### 3.2 `triple_custom.ini` (module "Display mode: Triple"; 5 header + 23 settings + 13 commented sample lines)

`[BASIC] ENABLED=1`; `[SCREEN_SPLIT] LEFT=0.333 RIGHT=0.333 FIT_SPLITS=1`; `[FSR_OVERRIDE] ACTIVE=0 QUALITY=0.5
SHARPEN=0.5 ADJUST_MIP_BIAS=0.5`; `[CAMERA_MODES_BEHAVIOR] COCKPIT=0 DRIVABLE=0 TRACK=0 ORBIT=0 CAR=0 FREE=0 START=0`
(0 default .. 4 enable centre part only); `[MISCELLANEOUS] CUSTOM_DRIVER_NAMES_PROJECTION2=1 (hidden)
ALWAYS_SHOW_DRIVER_NAMES=0`; `[VERTICAL_MARGINS] LEFT=0,0 CENTER=0,0 RIGHT=0,0`; commented `[CAMERA_LEFT/CENTER/RIGHT]`
(`MODE`, `POSITION`, `LOOK`, `UP`, `FOV`, `ASPECT_MULT`). Reader `0xe8b0a0`; module registration `0x26dd50`
(string "triple_custom" at `0x2276df8`) `[ini][dll]`.

### 3.3 `custom_rendering_modes.ini` (5 header... 16 live keys)

`[BASIC] DEBUG_MODE=0`; `[MODE_360] FRAME_RESOLUTION=1024 FRAMES_MARGIN=0.1`; `[MODE_FISHEYE] FRAME_RESOLUTION=1024
FRAMES_MARGIN=0.1 MODE=1 SCALE_LINEAR=0.6 SCALE_ANGLE=1`; read but not shipped `[MODE_SPLITSCREEN] HORIZONTAL_LAYOUT
TWO_RAW_MICE` (strings `0x2261110`, `0x22610f8`, `0x2261128`) `[ini][dll]`. `spectating.ini`: `[BASIC] ENABLED=0`
only; the RTTI name `ac_ext_spectating_settings` (`0x2625c88`) and a hidden key `SWITCH_CAMERAS` (`0x2273ed0`)
exist; consumer not found.

### 3.4 `dxgi_tweaks.ini` (37 live key lines = 3 header + 34 settings, plus 9 commented)

| Section | Keys (default) |
|---|---|
| `[COMMON]` | `SELECT_ADAPTER=0`, `BACKGROUND_PAUSE=0` (0 off / 2 = 30 FPS / 1 = 2 FPS / 4 pause), `EFFICIENCY_MODE=1`; commented `DPI_AWARE3=2`, `BACKGROUND_COLOR`, `HIDDEN_LOADING=0`, `ACCELERATED_EXIT2=1` |
| `[COMPATIBILITY]` | `OLD_WINDOW=0`, `OLD_SWAPCHAIN=0`, `EXCLUSIVE_FULLSCREEN=0`, `ALLOW_STRETCHING=0`, `OLD_STRETCH=0` |
| `[BASIC]` | `ENABLED=1` (no-op), `ADVANCED_PACING=0`, `MAXIMUM_LATENCY_ADVANCED=2`, `MAXIMUM_LATENCY_BASE=0`, `VSYNC_MODE=0`, `VSYNC_ADAPTIVE=0`, `ALLOW_VRR=2`, `FORCED_VSYNC_AWARE=0`, `SAVE_WINDOW=1`; commented `ALLOW_TEARING=1` |
| `[WINDOW_TWEAKS]` | `CONFINE_CURSOR=0`, `SET_TOPMOST=0`, `FORCE_FOCUSED=0`, `BORDERLESS_WINDOWED=0` |
| `[HDR]` | `ENABLED=0`, `VERIFY=1`, `UI_GAMMA=0.45`, `UI_BRIGHTNESS=1.7` |
| `[HDR_YEBIS]` | `USE_FILTER_VALUES=1`, `FORCE_LINEAR_FUNCTION=1`, `BRIGHTNESS_ADJUSTMENT=0.2`, `POST_TONEMAPPING=1`, `POST_GAMMA=0.45`, `POST_BRIGHTNESS=1.2`; commented `[HDR_DIRECT] SCENE_BRIGHTNESS`, `SCENE_GAMMA` |

Related `general.ini` keys: `PREVENT_SLEEP=2` (hidden: window flag asking Windows to stay awake),
`CONSIDER_OCCLUDED_FLAG=1`, `GRACEFUL_START=3` (skip rendering the first N frames), `DX_DEVICE_PRIORITY=7`,
`GRAPHICS_LEAD_FRAMES=0`, `PHYSICS_LEAD_TIME=0`, `[THREAD_TWEAKS]` (6 keys). `video.ini` keys CSP writes:
`[VIDEO] _EXT_PLACEMENT` (section 1.5), `[CUBEMAP] __ORIG_*`; files CSP writes under Documents:
`cfg\extension\window_position.ini` `[ini][log-csp]`.

### 3.5 Lua API groups (from `internal/lua-sdk/*/lib.lua`; counted in `lua_sdk_functions_union.txt`)

| Group | Names |
|---|---|
| VR (7 functions; `ac.getVR` is in 18 of the 19 SDK modules, not `ac_splashscreen`) | `ac.getVR`, `ac.recenterVR`, `ac.resetVRPose` (deprecated), `ac.setVRHandBusy(hand, busy)`, `ac.setVRHandVibration(hand, frequency, amplitude, duration)`, `ac.setVRSConfigurationForVR`, `ac.setVRSGazeForVR`; types `ac.StateVR`, `ac.StateVRHand`, enum `ac.VRSRateMode` (12 values); `ac.CSPModuleID.VRTweaks` |
| Triple (5) | `ac.getTripleConfiguration`, `ac.onTripleConfigurationChanged`, `ac.isTripleModeActive`, `ac.getTripleScreenViewport`, `ac.setTripleScreenSideValues`; types `ac.StateTriple`, `ac.StateTripleItem`; `ac.CSPModuleID.TripleCustom`; `render.ProjectFace.Auto` |
| Mode / display state (`ac.getSim()`, ~26 fields) | `isVRMode`, `isOculusMode`, `isOpenVRMode`, `isOpenXRMode`, `isVRConnected`, `isTripleMode`, `isCustomVideoMode`, `isTripleFSRActive`, `isFSRActive`, `isTAAActive`, `windowWidth/Height`, `windowSize`, `renderSize`, `isFullscreen`, `isHDR`, `isVSyncActive`, `isPostProcessingActive`, `msaaSamples`, `headsetPosition/Look/Up/Side`, `cameraFOV` (always 90 in VR), `isTripleScreenAvailable`; `ac.getUI().vrController`; render event `main.hud.vr` |
| Window / pacing (8) | `ac.toggleFullscreen`, `os.tweakMainWindow({command='activate'|'topmost'|'title'})` (I/O scripts only), `ac.disableFrameLimiter`, `ac.applyRenderingLimits({fpsLimit, renderScale, shadowResolution, extraFXAllowed})`, `ac.getPowerState`, `ac.onPowerStateUpdate`, `ac.refreshVideoSettings`, `ac.setDriverEyesPosition`; PP-filter scripts also `ac.setPixelDensity`, `ac.setMIPs` |
| Splitscreen support | `ac.loadCarControlsConfiguration`, `ac.setTargetCar`, `ac.applyLiveConfigEdit` (used by `mode.lua`) |

## 4. Files it reads

* `extension/config/{vr_tweaks,triple_custom,dxgi_tweaks,custom_rendering_modes,spectating,data_manifest}.ini`
  (+ owner overrides in `Documents\Assetto Corsa\cfg\extension\`; none exist for this area on this PC).
* acs.exe's own files: `cfg/video.ini` (`[CAMERA] MODE`, `[VIDEO]` size / refresh / `FULLSCREEN` / `AASAMPLES` /
  `FPS_CAP_MS` / `_EXT_PLACEMENT`, `[POST_PROCESS]`), `cfg/race.ini`, `cfg/triple_screen.ini`, `cfg/oculus.ini`,
  `cfg/openvr.ini`, `cfg/trackir.ini` `[prior][dll]`.
* `extension/internal/plugins/openxr_loader.dll`, `plugins/AcTools.ErrorReporter.exe`; the runtime's own
  `LibOVRRT*`, `openvr_api.dll` (AC root; `system/x64/openvr_api.dll` is probed for OpenComposite), the OpenXR
  runtime named by `XR_RUNTIME_JSON` / the system's active runtime; Revive DLLs from `REVIVE_LOCATION`.
* Oculus store manifests under the Oculus install (`Oculus/CoreData/Manifests`, `.../Software/StoreAssets`) with
  `UPDATE_MANIFEST` (written, not only read).
* Windows: registry `Software\Microsoft\DirectX\UserGpuPreferences`, monitor topology (`QueryDisplayConfig`),
  `user32` / `shcore` / `dwmapi` DPI and window APIs, ADLX (`amdadlx64.dll`), NVAPI (`nvapi64.dll`).
* Embedded in `dwrite.dll` (zip 1): the `acc*` shaders named in section 1, none encrypted.
* Lua: `extension/lua/new-modes/splitscreen/{manifest.ini,mode.lua,settings.ini,description.txt}` (plain text, 4,839 B),
  `lua/pp-filters/curved` (reads `triple_screen.ini`).

## 5. How it is switched on

| Feature | Switch | Default / this PC |
|---|---|---|
| Oculus | video.ini `[CAMERA] MODE=OCULUS`, or `OCULUS_TWEAKS AUTO_ENABLE=1` + launched from the Oculus runtime ("Launched from Oculus runtime, switching to VR mode") | off (`MODE=DEFAULT`) |
| OpenVR | `MODE=OPENVR` | off |
| OpenXR | launcher choice `__EXT_OPENXR`; acs.exe is told OPENVR (log "OpenXR requested; ACS will construct StereoCameraVive") | off |
| vr_tweaks module | no real switch (`ENABLED` is a no-op; `ENABLED2=0` unsupported) | present, inert without a headset |
| Triple | `MODE=TRIPLE`; `triple_custom [BASIC] ENABLED=1` | off |
| 360 / fisheye / split screen | launcher choice `__EXT_360` / `__EXT_FISHEYE` / `__EXT_SPLITSCREEN` (or a new mode with `RENDER_MODE=__EXT_SPLITSCREEN`); needs post-processing + FXAA | "custom mode is not selected" |
| DXGI Tweaks | gated: off with MSAA without post-processing; the module's hidden `ENABLED=1` is a no-op | **off** on this PC (log) |
| DPI awareness, window placement, `_EXT_PLACEMENT` | always, independent of the module | active |
| Per car / track | none | - |

## 6. Size

* Hook rows: 26 hooks + 6 lookups in group `vr_screens`, +1 (`OculusTexture::Init`) +3 (`TripleScreenManager`) +1
  (`FormCamera::FormCamera`) in other groups = 31 hooks; 6 module-qualified `openvr_api` hooks, 1 `ovr_*` hook (+13
  lookups), 2 VR `MessageBoxA` hooks, ~12 window / device / present entry points (section 2.3).
* Replacement code: VR+triple hooks 14,515 B measured (24 functions) + two `normalRendering` bodies; OpenXR cluster
  142 functions / 79,360 B; stereo hook region `0x11c6000..0x11cc000` 74 functions / 23,220 B; triple region
  `0x10a7900..0x10ab000` 31 functions / 13,991 B; window cluster `0x3ee180..0x3f8570` ~25 functions.
* Config: 4 ini files = 163 + 28 + 16 + 37 key lines (+ spectating 1). Lua SDK: 7 VR + 5 triple + 8 window / pacing
  functions, 3 types, 1 enum. Shipped Lua for split screen: 1 mode (4 files, 4,839 B).
* Acs.exe side for scale (`docs/atlas/renderer.md`): VR + triple 84 functions / 35,321 B (`CameraTripleScreen`,
  `StereoCameraForward`, `StereoCameraForwardVive`, `OculusManager`, `ViveManager`); `TripleScreenManager` +
  `VirtualDisplay`: 29 functions / 14,353 B; `libovr.lib` shim 8,057 B.

## 7. Port difficulty for rustyAC

Overall **XL** for VR parity, **M** for triple screen and the window / swap-chain layer, **L** for split screen,
**M** for 360 / fisheye. Reasons:

| Part | Size | What exists in rustyAC | What is hard |
|---|---|---|---|
| Window, DPI, flip-model swap chain, vsync | M | `crates/rustyac-game/src/window.rs` (Win32 window, borderless full screen or `--windowed`, per-monitor-v2 DPI), `render/ac.rs` (flip-discard 2-buffer chain through `CreateSwapChainForHwnd`, `--vsync`) | monitor topology + placement rules + saved `_EXT_PLACEMENT`, tearing / VRR / waitable pacing, HDR output, adapter choice, background pause, topmost / confine / borderless styles, "proxied output" stretch |
| Triple screen camera | M (acs's own) + S..M (CSP extras) | camera / forward / shadow-mapped renderers (`rustyac-render`), HUD triple places (proven, `gui_hud.md`), `CameraForwardYebis`-style passes | `CameraTripleScreen` itself is **not ported** (`postfx.md`: "Triple screen and VR ... not ported"); 3 virtual screens with their own multisampled targets and FXAA; custom controller cameras; side-screen FSR needs an upscaler (none in rustyAC) |
| Triple screen app | S | widget library complete (`gui_hud.md`) | `TripleScreenManager` / `VirtualDisplay` not ported (`apps_user.md`: triple screen not done) |
| VR minimal (one OpenXR session, two eyes, pose, submit, mirror) | L | `openxr` crate fits (`docs/atlas/renderer.md` section 6: "openxr crate replaces both libovr and OpenVR"); no OpenVR shim is needed in a rewrite | per-eye frustums, session / swap-chain lifecycle, GPU-adapter LUID, hidden-area mask, head-relative camera, HUD plane (`GUI::render3D` not ported) |
| VR parity (SPS, VRS, depth layers, passthrough, hands, eye tracking, controllers in cars, mirror modes + stabiliser, VR HUD styles) | XL | nothing | NVIDIA-only VRWorks via NVAPI, instancing shader variants of every material, interaction profiles, action bindings, car touch screens |
| Split screen | L | several-car engine (one world, a working copy per car; `docs/port/several_cars.md`), AI / replay, multi-car drawing | two viewports and HUDs, two control sets incl. raw mice, per-car assist state |
| 360 / fisheye | M | six cube faces already rendered for reflections | six full scene passes at 256-3072 px + a remap shader (standard mapping functions) |

What can be proven without a headset:
1. **Window placement**: a pure function of (monitor rectangles, DPI, requested size, saved rectangle). CSP logs
   both the inputs and the decision ("Detected monitors", "Placing AC window ..."), so run acs.exe + CSP under
   several synthetic layouts and compare the log lines and the final rectangle with rustyAC's function. One real
   data point is in hand (section 1.5 step 4).
2. **Swap-chain description / pacing**: CSP's Render Stats performance report lists swap chain format, flags,
   buffer count, V-sync / VRR state, max frame latency, fullscreen model. Compare those lines (text oracle) with
   rustyAC's `GetDesc1`.
3. **Triple screen**: run acs.exe with `video.ini [CAMERA] MODE=TRIPLE` (acs.exe alone for AC's own passes; with CSP
   for `triple_custom` keys) and compare the three viewport rectangles and frames. The split arithmetic is
   `int(W*LEFT)`, `int(W*RIGHT)`, rest; margins and FIT_SPLITS are numeric; `ac.getTripleConfiguration()` via a
   small Lua app prints `left` / `right` / `height` for every screen.
4. **Eye-offset math**: unit-level: translate by +-0.5 x EXTRA_EYES_MARGIN_MM on the eye matrix; verify by feeding
   CSP the same synthetic eye matrices through an OpenVR null driver (SteamVR "null" HMD is the only no-hardware
   path to the OpenVR camera [interp: not present on this PC]) and compare the submitted eye textures.
5. **Config surface**: every key in 3.1..3.4 can be checked for parse / default by a Lua probe
   (`ac.INIConfig`) since CSP does not log the values.
6. **Custom modes**: image-compare 360 / fisheye frames (post-processing + FXAA on) from acs.exe + CSP against
   rustyAC; the mapping functions are textbook.
7. **Not provable**: Oculus (needs the Oculus service), VRS, single-pass-stereo performance, real OpenXR
   runtimes, controllers. A software OpenXR runtime (Monado / simulator runtimes) could exercise the OpenXR path
   if one is installed [interp].

## 8. Open questions and not mapped

1. **`0x11e82b0`** (VR / mode detection, 10 KB) and **`0xe94a80`** (vr_tweaks reader, 14 KB) are not decompiled:
   which globals each key lands in is unknown beyond the ones named here (`0x182693970` SPS, `0x182695130`
   extra eye distance, `0x18268c9a8/9a9` submit mode / submit depth, `0x1826a4905` OpenXR flag, `0x1825e8d68`
   "skip frame" flag).
2. The original bodies of `OculusManager::update` and `ViveManager::update` (acs.exe, 99 / 251 B) are not in
   `docs/`; what exactly CSP dropped (audio device autoselect? recenter on reset?) is not stated.
3. The OpenXR cluster is mapped by strings only (roles in 1.2); the OpenVR shim's object layouts
   (IVRSystem_015 / IVRCompositor_020 / IVRSettings_002 method coverage) are unread.
4. `custom.comparison`: a fourth custom mode id with no ini / manifest / Lua; consumer and meaning unknown.
5. The split-screen C++ part (second viewport, second control set, raw-mouse handling): not located; likely
   under `0xf95550` / `0xc6e130` neighbours and `Sim` hooks.
6. `[CAMERA_LEFT/CENTER/RIGHT]` parser, `CAMERA_MODES_BEHAVIOR` consumer: not located (camera managers).
7. `spectating.ini` / `SWITCH_CAMERAS` consumer not found (no `lea` reference).
8. `acsW_secondary` (UTF-16 class name, function `0x378b40`; methods setTopmost / setFullscreen / setClientSize /
   monitors) is CSP's own second OS window class; no Lua API for it appears in the SDK. Purpose not mapped.
9. NVAPI use: single-pass stereo, VRS and NVIDIA profile reading go through `nvapi_QueryInterface` with numeric
   ids; which ids, and whether CSP *writes* the driver profile (the report only says "NVIDIA profile V-sync"),
   is unknown.
10. `__EXT_OPENXR` -> `OPENVR` rewrite: whether CSP edits race.ini on disk or only its own copy (the INIReader
    hooks answer from memory, attach.md 1.8) is not decided.
11. Steam VR streaming helpers (`isVRHeadsetStreamingEnabled`, `setVRHeadsetStreamingEnabled`, string
    `0x21f9c38`) belong to the Steam hooks (attach.md 1.8); purpose in AC unknown.
12. `OCULUS_IN_VR` (UTF-16, `0x2297928`): probably an environment variable set by the Oculus launcher; unverified.

## Evidence

* `re/scratch/csp1/hook_groups.tsv` (32 `vr_screens` rows), `csp_hook_table.tsv`, `decomp_hooks/` 4148e0, 6bb0c0,
  10a7990, 10a8c70, 10a99a0, 11c6010, 11c6650, 11c69c0, 11c6d80, 11c6da0, 11c6fa0, 11c7060, 11c7100, 11c8180,
  11c8270, 11c8f90, 11c9300, 11c9520, 11c9540, 11c9710, 11c9930, 11c99d0, 11c9c90, 11ca4d0, 11ca710, 11cb110,
  11cbaa0, 5dbb90, 6a7f70, 6a7f90, 6a7fa0; `decomp_infra/c73560.c`; `decomp_req1/{6c3f50,6c5170,1387350}.c`.
* Registration-code decode (read-only byte reads of `dwrite_copy.dll`): regfns 0x2661d0, 0x2663a0, 0x266490,
  0x266580, 0x266670, 0x266760, 0x266850 (openvr_api), 0x22c680, 0x22c790, 0x22c870, 0x22c960, 0x22ca50, 0x260880,
  0x260970, 0x260b50, 0x283430, 0x2a5250, 0x2a55e0, 0x2affb0, 0x2b0170, 0x2af0b0 (pHMD), 0x2add90 (hLibOVR); stub
  0x11cbfa0 (call through slot 0x26ade80).
* `dll_strings.tsv` / `dll_func_strings.tsv` / `dll_funcs.tsv` ranges 0x21fbf00..0x21fcb60 (window), 0x2222a00..
  0x2223f00 (DXGI), 0x222a8e8..0x222cf00 (OpenXR / OpenVR), 0x2226600..0x2226800 (OpenComposite, ovr),
  0x2261000..0x2261600 (custom modes, DPI), 0x2277300..0x2277800 (vr_tweaks keys), 0x2293b00..0x2294fff (Oculus /
  OpenVR labels), 0x2297900..0x2297e00 (start-up), 0x223b700..0x223e400 (Render Stats report), 0x2267f00..0x2268200
  (render-mode ids).
* `<AC>/extension/config/{vr_tweaks,triple_custom,dxgi_tweaks,custom_rendering_modes,spectating,data_manifest,
  data_alt_mapping,graphics_adjustments,small_tweaks,smart_mirror,neck,general,particles_fx,gui,splashscreen}.ini`;
  `<AC>/extension/lua/new-modes/splitscreen/*`; `<AC>/extension/lua/pp-filters/curved/filter.lua`;
  `<AC>/extension/internal/lua-sdk/ac_apps/lib.lua`, `ac_pp_filters/lib.lua`; `internal/plugins/openxr_loader.dll`.
* `Documents\Assetto Corsa\logs\custom_shaders_patch.log`, `cfg\video.ini`, `cfg\triple_screen.ini`,
  `cfg\extension\window_position.ini`.
* `acs_index/functions.tsv`, `acs_index/func_strings.tsv` (interface versions, `ipdOffset`, `renderTargetMultiplier`,
  `cfg/oculus.ini`, `cfg/openvr.ini`); `docs/atlas/{renderer,camera,config,ui}.md`; `docs/port/{gui_hud,postfx,
  apps_user}.md`; `crates/rustyac-game/src/{window,cli}.rs`, `render/ac.rs`; `attach.md` 1.4, 1.5, 1.7, 1.8, 2;
  `ui_apps.md` 2.4; `rendering_core.md`; `input_ffb.md` 1.9; `audio.md`.

## Decompile requests

1. `0x11e82b0` (10,095 B): VR / TRIPLE / custom-mode detection, `__EXT_OPENXR` rewrite, Revive injection, AUTO_ENABLE.
2. `0xe94a80` (14,042 B): vr_tweaks.ini loader; map every key to its global (SPS flag, eye margin, submit mode).
3. `0x6f3060`, `0x6f31b0`, `0x6f31d0`, `0x6f3230`, `0x6f32d0`, `0x6f3370`: the OpenVR shim exports; which interface
   versions and methods are served.
4. `0x6f2610`, `0x6f26b0` (IVRSettings proxy: `ipdOffset`, `renderTargetMultiplier`) and `0x11ca5a0`, `0x11ca630`.
5. `0x6eb440` (3,585 B), `0x6e8b80`, `0x6ea6e0`, `0x6e80e0`, `0x6e7160`: OpenXR instance / system / session /
   swap-chain set-up; which extensions are required vs optional.
6. `0x6ee0b0`, `0x6eeca0`, `0x6ef430`, `0x6efe90`, `0x6f09d1`, `0x6f28b0`: OpenXR eye / depth / HUD layers and
   `xrEndFrame`.
7. `0x6e6100` (3,589 B), `0x6e3560`, `0x6e3f40`, `0x6e49c0`: input actions and profile bindings.
8. `0x11c7980` (2,041 B): the Oculus present; `0x11cb460` (1,596 B): VR loading image / cubemap; `0x11cad90`.
9. `0x6db240`, `0x6ddcf0`, `0x6dda90`, `0x6dd5e0`, `0x6f0aa0`: OpenComposite detection, hidden-area mask, VR cover.
10. `0xe8b0a0` (4,455 B): triple_custom loader; `0x10a8680`, `0x10a79c0`, `0x10aa760`: custom-camera calculation and
    constructor helper; the creator of the object at `0x1826ab110`.
11. `0xf95550` (1,852 B), `0xc6e130` (1,517 B), `0xcf3410` (1,716 B): custom-mode loader, remap pass, mode names;
    and the function that renders the six / five sub-frames and the split-screen viewports.
12. `0x3f30b0`, `0x3f3860`, `0x3f5ed0`, `0x3f40d0`, `0x3f13b0`, `0x3eea40`, `0x3ef280`, `0x3f2c60`, `0x3ee180`,
    `0x3f5690`: window placement, creation and `_EXT_PLACEMENT` encoding.
13. `0xc71990`, `0xc72f70`, `0xf8c6e0`, `0x1509f10`: dxgi_tweaks key readers and where CONFINE_CURSOR / TOPMOST /
    FORCE_FOCUSED are applied.
14. `0x6c10d0`, `0x6c2760`, `0x6c39a0`, `0x6c69b0`, `0x6c62e0`, `0x6c64c0`, `0x117dcb0`, `0x117d670`: adapter
    choice, VRR detection, present, forced-V-sync logic, resize / proxied output.
15. `0x378b40`: the `acsW_secondary` window class (what uses it).
