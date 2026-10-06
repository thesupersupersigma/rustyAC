# Track: loading, special nodes, timing lines, track-side objects

Source: `re/decomp/` pseudo-C (folders `TrackAvatar`, `TrackObject`, `StartingLights`, `StartingTrackSemaphore`,
`DynamicTrackManager`, `SurfacesManager`, `ConstrainedObjectsManager`, `RotatingObjects`, `MicroSectors`, `IdealLine`,
`TimeLine`, `TimeTransponder`, `Track`, `DRSManager`, `Spline`, `InterpolatingSpline`, `TrackMap`), `re/types/*.txt`,
`tools/re_query.py` (find / callers / callees / strings / uses / vtable / size) and a read-only listing of
`content/tracks/spa` and `content/tracks/ks_nordschleife`. Nothing in the game folder, the Ghidra project or git was
changed. "Confirmed" = read in the pseudo-C or the string index; "interpretation" is marked as such.
The physics surface (collision meshes, `surfaces.ini` keys, ground ray, grip) is in `docs/map/track_surface.md` and is
not repeated here.

## 1. What it is

A track is one `GameObject` called `TrackAvatar`, built once by `Sim::loadTrack` while the splash screen is up.
It loads one or more `.kn5` model files into the scene graph, then walks the node tree three times: once to turn every
mesh whose name starts with a number into a physics surface (see `track_surface.md`), once to find helper nodes with
fixed `AC_...` names (grid slots, pit boxes, timing gates, movable objects, pit crew spots, audio emitters), and once to
build timing lines from pairs of left/right gate nodes.
Everything else about a track is small ini files in `content/tracks/<track>[/<layout>]/data/` plus two binary splines
in `ai/` (the racing line and the pit lane), which are owned by the physics-side `Track` object embedded in
`TrackAvatar`.
Spawn positions are just scene nodes: the loader finds `AC_<SET>_<n>`, drops each one onto the road with a ray cast and
keeps them in a map keyed by set name (`PIT`, `START`, `TIME_ATTACK`, `HOTLAP_START` ...); cars ask for "set, index".
Lap timing is done on the physics thread: each car has a `TimeTransponder` that tests one wheel position against every
`TimeLine` each 3 ms step; line 0 is start/finish, the others are sector splits.
The rest are independent small objects hung on the main-loop tree: start lights, the track semaphore, the visual
rubber groove, movable cones/signs, the ideal-line ribbon, micro-sector colours, the track map app.
Position along the lap ("normalized spline position", 0..1) is used everywhere (cameras, map, DRS zones, AI,
leaderboard gaps); it comes from the `kml.lib` spline classes that back the AI line.

## 2. Main classes

| Class | Source file (library) | Size | Role |
|---|---|---|---|
| `TrackAvatar` | TrackAvatar.obj | 0x308 | The track `GameObject`. Owns `model`, the physics `Track` (embedded at +0xf8), `SurfacesManager`, `spawnPositions` map, `trackObjects`, `dynamicObjects`, `lollipopCrews`, `idealLine`, `dynamicTrackManager`, `sectorDescriptions`, sun location. |
| `Track` | Track.obj (ksPhysicsAC.lib) | 0x148 | Physics side: surfaces, `timeLines`, `DRSManager`, `aiSplineRecorder`, dynamic grip. Mapped in `docs/map/track_surface.md`; counted there. |
| `SurfacesManager` | SurfacesManager.obj | 0x20 | Map of surface name -> `SurfaceDef` from `surfaces.ini`; resolves a mesh name to a surface. Detail in `track_surface.md`. |
| `TimeLine` | TimeLine.obj (ksPhysicsAC.lib) | 0x38 | One timing gate: two end points, `length` (squared gate width), `planeNormal`, `type` (Default / ABStart / ABFinish), `id`. |
| `TimeTransponder` | TimeTransponder.obj (ksPhysicsAC.lib) | 0x98 | Per car: lap timer `t` (ms), `lastLap`, `bestLap`, `lapCount`, split vectors, `cuts`, open-track state. Car physics; not counted here. |
| `TrackObject` | TrackObject.obj | 0xd8 | One movable object (`AC_POBJECT...` node): creates a `PhysicsObject` from the mesh, receives its matrix through a `BufferedChannel<mat44f>` and applies it to the node. |
| `DynamicTrackObject` | (struct in TrackAvatar.obj) | 0x38 | Decorative moving model (node, pos, range, velocity): position += velocity * dt each frame. |
| `StartingLights` | StartingLights.obj | 0xf0 | Start-light sequence: HUD lights plus emissive `KS_START_LIGHT_n` objects on the track. |
| `StartingTrackSemaphore` | StartingTrackSemaphore.obj | 0x88 | Track-side semaphore objects driven by `data/semaphore.ini` (groups READY / START / DISQUALIFIED). |
| `DynamicTrackManager` | DynamicTrackManager.obj | 0x90 | Graphics side of the rubbered-in line: fades the `GROOVE_n` meshes in by setting their `alpha` shader variable. |
| `IdealLine` | IdealLine.obj | 0xe8 | Scene node with the driving-line ribbon mesh built from an `AISpline` (`SplineStripBuilder`). |
| `MicroSectors` | MicroSectors.obj | 0x90 | Per-car best times over small lap slices; draws the coloured sector bars in leaderboard / performance apps. |
| `Spline` | Spline.obj (kml.lib) | 0x28 | Poly-line: `points` (`SplinePoint`: position, length, tag), `m_length`, `m_closed`; closest-point search, CSV load. |
| `InterpolatingSpline` | InterpolatingSpline.obj (kml.lib) | 0x70 | `Spline` + interpolation (linear, Catmull-Rom, cubic, Bezier, B-spline), a 2D lookup grid for fast world->spline, world<->spline conversion. |
| `ConstrainedObjectsManager` | ConstrainedObjectsManager.obj | 0x78 | Belongs to a **car** (`CarAvatar &car`), not the track: keeps `DIR_...` nodes pointed at target nodes (suspension arms, steering links). Listed here because it was assigned to this file. |
| `RotatingObjects` | RotatingObjects.obj | 0x78 | Also car-side: spins nodes listed in the car's `data/extra_animations.ini`. Not counted here. |
| `DRSManager` | DRSManager.obj (ksPhysicsAC.lib) | 0x38 | DRS zones (`detection`, `start`, `end` as lap fractions); counted with aero. |
| `TrackAudio` | TrackAudio.obj | - | One line: `AC_AUDIO_n` emitters, reverb zones and occluders from `data/audio_sources.ini` (audio area). |
| `TrackMap`, `TrackMapHandler` | SystemApps.lib | - | Track-map app: loads or generates `map.png` + `data/map.ini` (UI area). |

## 3. Entry points

### 3.1 Creation (main thread, during loading)

`Sim::Sim` 0x140192070 -> `Sim::loadTrack` 0x14019a4c0 -> `TrackAvatar::TrackAvatar` 0x1401c5250, which runs, in
this order (confirmed from call order):

1. `TrackAvatar::init3D` 0x1401c8740. Opens `content/tracks/<track>/models.ini` or `models_<layout>.ini`; each
   `[MODEL_n]` has `FILE`, `POSITION`, `ROTATION` and is loaded with `Model::load` 0x140217d30 / `KN5IO`. Without a
   models file it loads `content/tracks/<track>/<track>.kn5`; for a layout the models file is mandatory
   (`ERROR, models_[config].ini file is required...` then a deliberate crash). Texture override folders come from
   `system/cfg/track_skins.ini` (`[FOLDER_n] NAME` -> `<track>/skins/<name>`).
2. `TrackAvatar::initPhysics` 0x1401ca440: `TrackAvatar::processPhysicsNode` 0x1401cc5e0 over the node tree
   (-> `TrackAvatar::addPhysicsMesh` 0x1401c78e0), then `Track::initAISpline` 0x1402782a0 with the `<data>/../ai`
   folder (AI line, see `ai_drivers.md`) and `Track::initStartingBounds` 0x140278790.
3. `TrackAvatar::initDynamicObjects` 0x1401c9840: `[DYNAMIC_OBJECT_n]` sections of the same models file (`FILE`,
   `PROBABILITY`, `POS_MODE`, `RND_POS_CENTER`, `RND_POS_RANGE`, `VEL_MODE`, `RND_VEL_BASE`, `RND_VEL_RANGE`), loaded
   with `KN5IO::load` 0x1402151a0. Interpretation: balloons, birds and similar scenery.
4. `TrackAvatar::initRespawnPositionSet` 0x1401cb8c0 three times: `PIT`, `START`, `TIME_ATTACK`.
5. `TrackAvatar::initTimeLines` 0x1401cbc90 -> `Track::addTimeLine` 0x140278040.
6. Ideal line: `data/ideal_line.ai` if it exists (`AISpline::loadFast` 0x1402a83c0), otherwise the AI recorder's
   best-lap spline; -> `IdealLine::IdealLine` 0x140109d10.
7. `data/lighting.ini` `[LIGHTING] SUN_PITCH_ANGLE, SUN_HEADING_ANGLE`; console variables `sunHeading`, `sunPitch`.
8. `DynamicTrackManager::DynamicTrackManager` 0x1401ccc20, `TrackAudio::TrackAudio` 0x1401c24c0,
   `CameraFacing::CameraFacing` 0x14005e640 (`data/camera_facing.ini`, crowd billboards).
9. `data/data.ini` `[POSITION] LATITUDE, LONGITUDE, GMT, NORD_OFFSET` (sun position), `data/sections.ini`
   `[SECTION_n] IN, OUT, TEXT` (corner names for `TrackDescription::render` 0x1403273c0).
10. Every node whose name contains `AC_POBJECT` -> `TrackObject::TrackObject` 0x1401cf1a0 (builds a
    `PhysicsObjectDesc` from the mesh vertices, `mass = 1.0`, -> `PhysicsObject::PhysicsObject` 0x1402ac8a0).
11. Handler on `Sim::evOnNewSession` (resets dynamic objects), prints `TRACK LENGTH` from the AI spline.

Later in `Sim::Sim`: `TrackAvatar::initPitCrew` 0x1401ca630 (after cars exist), `StartingLights::StartingLights`
0x1401a1a20, `StartingTrackSemaphore::StartingTrackSemaphore` 0x1401a37a0, `MicroSectors::MicroSectors` 0x14009e9d0,
`TrackMap::TrackMap` 0x1402ee8e0. `RaceManager::initOffline` 0x14013a6c0 calls `initRespawnPositionSet` again for the
`SPAWN_SET` of every `[SESSION_n]` in `race.ini` (default `PIT`).

### 3.2 Special node names in the kn5 (confirmed from string literals)

| Node name | Used by | Meaning |
|---|---|---|
| `<number>NAME...` (mesh) | `NKUtils::getSectorID` 0x14018d2b0, `SurfacesManager::getSurface` 0x1401af340 | Physical surface; see `track_surface.md`. Names starting with `AC_` are skipped by `processPhysicsNode`. |
| `AC_<SET>_<n>`, n = 0,1,2... until missing | `initRespawnPositionSet` | Spawn slot. Sets seen: `AC_PIT_n` (pit boxes), `AC_START_n` (grid), `AC_TIME_ATTACK_n`, and any `SPAWN_SET` from race.ini, e.g. `AC_HOTLAP_START_n`. The node is moved onto the ground: ray from 10 m above, straight down, 100 m long, through `Track::rayCast` 0x140278bb0. |
| `AC_TIME_<n>_L` / `AC_TIME_<n>_R` | `initTimeLines` | Timing gate n. 0 = start/finish, 1.. = sector splits. Loop stops at the first missing pair. |
| `AC_AB_START_L/_R`, `AC_AB_FINISH_L/_R` | `initTimeLines` | Point-to-point track: adds an ABStart and an ABFinish line, prints `TRACK IS AN OPENTRACK`. |
| `AC_OPEN_FINISH_L/_R` | `initTimeLines`, `DragMode::DragMode` 0x140084a20 | Extra finish gate (drag strips). |
| `AC_POBJECT...` | constructor | Movable physics object (cones, signs). |
| `AC_CREW_<n>` | `initPitCrew` | Pit crew / lollipop position per pit box. |
| `AC_AUDIO_<n>` | `TrackAudio` | Ambient sound emitter. |
| `KS_START_LIGHT_<n>` | `StartingLights` | Start-light objects whose material gets `ksEmissive` switched. |
| `GROOVE_<n>` names from `groove.ini` | `DynamicTrackManager` | Rubber-line meshes. |
| `KSTREE...` | none | No such literal in hand-written code: trees are handled by kn5 materials/shaders only (interpretation). |

`TrackAvatar::getSpawnPosition` 0x1401c8200 (set name, index) is the single accessor; callers include
`CarAvatar::CarAvatar`, `CarAvatar::goToSpawnPosition` 0x1400d3ae0, `PitStop::PitStop`, `Sim::addNetCar`,
`TimeAttack::TimeAttack`, `DragMode`. `TrackAvatar::getMaxSlotsAvailable` 0x1401c8090 (set name) returns how many
slots that set has (confirmed: size of the node vector).

### 3.3 Per frame (main thread, `GameObject` tree)

- `TrackAvatar::update` 0x1401cca10: per car, sets one flag on its pit-crew figure from `ACClient::getPing`
  (interpretation: hides the crew of disconnected cars online); then `TrackAvatar::updateDynamicObjects` 0x1401ccb30.
- `TrackObject::update` 0x1401cf720: takes the newest matrix from the physics channel; replay playback writes
  matrices directly with `TrackObject::setMatrix` 0x1401cf6f0.
- `DynamicTrackManager::update` 0x1401cdba0: groove alpha from `Track::dynamicGripLevel` (`(grip - 0.9) * 10`) when the
  dynamic track is enabled, otherwise from the summed lap counts of all cars against `MAX_LAPS`.
- `StartingLights::renderHUD` 0x1401a2e80, `StartingTrackSemaphore::update` 0x1401a4cc0: both read
  `RaceManager::getTimeToSessionStart` 0x14013a0a0 (or the online session) and switch lights.
- `MicroSectors::update` 0x14009fe20; drawn by `MicroSectors::drawSectors` 0x14009f290 from leaderboard apps.

### 3.4 Per physics step (physics thread)

- `Car::stepComponents` 0x1402764d0 -> `TimeTransponder::step` 0x1402911f0: for every `TimeLine`,
  `TimeLine::check` 0x1402caef0 with the world position of tyre 0; a change from the negative to the positive side of
  the gate plane (and both gate ends within range) fires `TimeTransponder::onTimeLinePassed` 0x140290c20, then
  `TimeTransponder::lap` 0x140290a50 for line 0 or `TimeTransponder::split` 0x140291150 for the others. `t` grows by
  3 ms per step, except before the green light in a race. `lap` raises `Car::evOnLapCompleted`.
- `Car::stepComponents` -> `SplineLocator::step` 0x1402ab5c0 keeps each car's lap fraction and lateral offset.
- `DRS::step` 0x1402b4e60 -> `DRSManager::isDRSAvailable` 0x140279490 (zones can also arrive from the server:
  `DRSManager::setZones` 0x140279500).
- `Track::step` 0x140278d20 (grip evolution, AI line recorder): see `track_surface.md`.

## 4. What it reads from disk

Base folder `content/tracks/<track>/` and, for a layout, `content/tracks/<track>/<layout>/`
(`TrackAvatar::getDataFolder` 0x1401c8060 returns the `data` folder of the active layout).

| Path | Reader | Content |
|---|---|---|
| `<track>.kn5`, `models.ini` / `models_<layout>.ini` + listed `.kn5` | `init3D`, `initDynamicObjects` | Models (`[MODEL_n]`, `[DYNAMIC_OBJECT_n]`). kn5 format belongs to the renderer area. |
| `system/cfg/track_skins.ini`, `<track>/skins/<name>/` | `init3D` | Texture override folders. |
| `data/surfaces.ini`, `system/data/surfaces.ini` | `SurfacesManager::loadSurfaceDefinitions` 0x1401afad0 | Surface types (`track_surface.md`). |
| `ai/fast_lane.ai`, `ai/pit_lane.ai`, `data/ai_hints.ini`, `data/side_l.csv`, `data/side_r.csv` | `AISplineRecorder` | AI line (`ai_drivers.md`). |
| `data/ideal_line.ai` | `TrackAvatar` constructor, `RaceEngineer`, `TimeAttack` | Optional separate line for the visual ideal line and lap/fuel estimates. |
| `data/lighting.ini`, `data/data.ini` | constructor | Sun angles; latitude/longitude/time zone. |
| `data/sections.ini` | constructor | Named track sections by lap fraction. |
| `data/crew.ini` (`[HEADER] SIDE`), `content/texture/crew_*`, car `skin.ini` `[CREW] SUIT` | `initPitCrew` | Pit crew side and suits. |
| `data/drs_zones.ini` (`[ZONE_n] DETECTION, START, END`) | `DRSManager::DRSManager` 0x140278ea0 | DRS zones. |
| `data/groove.ini` (`[HEADER] GROOVES_NUMBER`, `[GROOVE_n] NAME, MIN, MAX, MULT`), `cfg/race.ini` `[GROOVE] MAX_LAPS, STARTING_LAPS` | `DynamicTrackManager` | Visual groove. |
| `data/startinglights.ini` (`[SETTINGS] INTENSITY, EMISSIVE, DIFFUSE`), `content/texture/off.png` | `StartingLights` | Start lights. |
| `data/semaphore.ini` (`INTENSITY`, `COLOR`, `GROUPS`, `[OBJECT_n] NAME, TYPE, ORDER`) | `StartingTrackSemaphore` | Track semaphore. |
| `data/cameras.ini`, `data/cameras_<n>.ini` | `CameraTrack::loadSet` 0x1400cb670 | TV cameras (camera area). |
| `data/camera_facing.ini`, `data/audio_sources.ini`, `data/overlays.ini` | `CameraFacing`, `TrackAudio`, `FlagManager` / `TimeAttack` | Crowds, audio, time-attack checkpoints. |
| `map.png`, `data/map.ini` (`[PARAMETERS] WIDTH, HEIGHT, MARGIN, SCALE_FACTOR, X_OFFSET, Z_OFFSET, DRAWING_SIZE`), `system/cfg/map.ini` | `TrackMap::loadMap` 0x1402ef590, `TrackMapHandler` | Track map; if missing, `TrackMap::createMap` 0x1402eeee0 draws it from the AI spline and writes both files. |
| `cfg/gameplay.ini` `[GUI] SHOW_SECTORS`, `cfg/assists.ini`, `content/texture/ideal_line.png` | `MicroSectors`, `IdealLine` | User options. |

`ui/` (preview, `ui_track.json`) is not referenced by `acs.exe` strings found here: it is launcher data (interpretation).
`data/sounds.ini` is a car file (`CarAudioFMOD`), not a track file.

## 5. Size

`python tools/re_query.py size TrackAvatar TrackObject StartingLights StartingTrackSemaphore DynamicTrackManager
SurfacesManager ConstrainedObjectsManager MicroSectors InterpolatingSpline Spline TimeLine IdealLine`:
**208 hand-written functions, 121,993 code bytes.**

| Object | Functions | Code bytes | | Object | Functions | Code bytes |
|---|---|---|---|---|---|---|
| TrackAvatar.obj | 38 | 38,738 | | InterpolatingSpline.obj (kml.lib) | 43 | 22,703 |
| SurfacesManager.obj | 5 | 10,956 | | Spline.obj (kml.lib) | 30 | 10,292 |
| MicroSectors.obj | 15 | 8,793 | | StartingTrackSemaphore.obj | 14 | 8,311 |
| StartingLights.obj | 15 | 8,218 | | DynamicTrackManager.obj | 13 | 5,803 |
| ConstrainedObjectsManager.obj | 6 | 3,194 | | IdealLine.obj | 9 | 2,490 |
| TrackObject.obj | 17 | 1,942 | | TimeLine.obj | 3 | 553 |

Not counted here (mentioned only): Track.obj, DRSManager.obj, TimeTransponder.obj, SplineLocator.obj (physics maps /
`ai_drivers.md`), TrackAudio.obj (audio), TrackMap*.obj, TrackDescription.obj, FormDRSZones.obj (SystemApps.lib, UI),
CameraTrack.obj and CameraFacing (camera), RotatingObjects.obj, PitCrew / LollipopCrew, SplineStripBuilder.obj,
KN5IO.obj / Model.obj (renderer), NKUtils.obj.

## 6. Port difficulty: **M**

- Logic is simple and data driven: ini files, node-name conventions, plane-crossing tests. The two long functions
  (`TrackAvatar::TrackAvatar`, `initPitCrew`) are mostly string building.
- It cannot start before the kn5 loader and scene graph exist (renderer area) and before the physics `Track`
  (`track_surface.md`); spawn sets need the ground ray cast.
- Must be exact for physics parity: `TimeLine::check` + `TimeTransponder` (lap times, 3 ms resolution, wheel 0 as
  the probe), spawn node snapping, and the `kml.lib` splines, because AI, `SplineLocator`, DRS zones and gaps all read
  the same normalized position. `InterpolatingSpline` (22 kB) is the only maths-heavy part: port it 1:1 rather than
  swapping in a spline crate, since the grid lookup and length computation define the lap fraction.
- Free to reimplement: start lights, semaphore, groove fade, dynamic scenery, pit crew, micro sectors, track map
  (an `image` crate can write `map.png`), ideal-line ribbon.
- Third-party: none directly (FMOD only through `TrackAudio`).
- Depends on: config (INIReader), renderer (kn5, materials, shader variables), physics engine (`PhysicsObject`,
  ray cast), session logic (`RaceManager` time to start), AI spline.

## 7. Open questions

- Which model is loaded when a layout has no models file was read from string order only; the exact fallback branch in
  `init3D` was not traced.
- `TrackObject` mass is the literal 1.0 in the descriptor; whether `PhysicsObject` overrides it from mesh size was not read.
- `KSTREE`: only the absence of the string is confirmed.
- `TimeLine::length` is the squared distance between the two gate nodes (confirmed in `TimeLine::TimeLine`
  0x1402cadb0). `TimeLine::check` uses 0 for the height of the tested point; whether `Track::addTimeLine` also flattens
  the gate nodes to y = 0 was not read.
- `MicroSectors` slice size and colour rules were not read.
- `semaphore.ini` and `startinglights.ini` key lists are from string literals; which keys are per section is partly guessed.
- Choice made: `ConstrainedObjectsManager` is counted here because the task assigned it, although it is car-side;
  `RotatingObjects` is left to the car-visuals area. `SurfacesManager` is counted here and only described in
  `track_surface.md`.
