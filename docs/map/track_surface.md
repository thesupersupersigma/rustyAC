# Track surface and ground contact

Source: `acs.exe` + `acs.pdb`, read through the local index only (`re/decomp/`, `re/types/`,
`tools/re_query.py`, `tools/disasm.py`). Folders read: `Track`, `SurfacesManager`, `TrackAvatar`,
`NKUtils`, `DynamicTrackManager`, `PhysicsCore`, `RayCaster`, `CollisionMeshODE`, `TyreTester`, and
the users of `SurfaceDef` in `Tyre`, `Car`, `SpeedLimiter`, `LapInvalidator`. Data read:
`system/data/surfaces.ini`, the 47 `content/tracks/**/data/surfaces.ini` files in the game folder,
`cfg/race.ini`. Raw pseudo-C of every function cited here: `re/car/track_surface/` (git-ignored).
Survey script: `re/scratch/track_surface/survey_surfaces.py`.
Nothing in the game folder, the Ghidra project, a running game or git was changed. No Rust was written.

"Confirmed" below means seen in the pseudo-C or the disassembly. Anything that rests on the ODE
0.13.1 source instead of the binary is marked "(ODE source)".

---

## 1. Plain-English summary

The physics side of a track is a pile of triangles plus a small label on each pile.
When a track loads, the game walks every 3D object of the track model. An object becomes
"physical" only if its name starts with a number, for example `1ROAD_3` or `2WALL`. The number
says which group of nearby objects it belongs to; the letters after it say what kind of ground it
is. The game looks those letters up in a list of surface types (`surfaces.ini`) and attaches a copy
of the matching entry to the object: how grippy it is, how bumpy, how dirty it makes the tyres,
whether it counts as "on track", whether it is pit lane, and how much it should shake the steering
wheel.
Each physics step every wheel shoots one line straight down, starting two metres above the wheel
centre. The answer is the point where the line meets a triangle, the direction that triangle
faces, and the label of the object it belongs to. The tyre does everything else itself.
Besides the labels, the track keeps one number for the whole circuit: the grip level. It starts
somewhere between 85 % and 100 %, goes up a little for every lap any car completes, and can carry
part of that gain into the next session. The tyre multiplies its grip by this number.
The car body does not use the wheel ray. Its underside boxes rest on the ground triangles and its
body shell bumps into walls through the ordinary collision system of the physics library.
Air temperature, road temperature and wind are not stored in the track; the physics engine object
owns them.

---

## 2. Classes

| Class | Size | What it is for |
|---|---|---|
| `Track` | 0x148 | The physics track. Owns the list of collision meshes (`surfaces`), timing lines, the dynamic grip state (`dynamicTrack`, `dynamicGripLevel`), the AI line recorder and the DRS zone manager. Implements `IRayTrackCollisionProvider`. Lives inside `TrackAvatar` (`physicsTrack` at +0xf8) and is also reachable as `PhysicsEngine::track` (+0x198). |
| `IRayTrackCollisionProvider` | 0x8 | Interface the tyre holds (`Tyre::rayCollisionProvider`, +0x5a8). Slots: +0x00 destructor, +0x08 `rayCast(org, dir, RayCastResult*, length)`, +0x10 `rayCastWithRayCaster(org, dir, RayCastResult*, length, IRayCaster*)`, +0x18 `createRayCaster(length)`. Implemented by `Track` and by `TyreTester`. |
| `IRayCaster` → `RayCaster` | 0x18 | A private ODE ray (`ray`) plus a pointer to the `PhysicsCore`. One per tyre (`Tyre::rayCaster`, +0x620). Slots: +0x00 destructor, +0x08 `rayCast(org, dir)` returning `RayCastHit`, +0x10 `release`. |
| `RayCastHit` | 0x28 | What the physics library wrapper returns: `pos` (+0x0), `normal` (+0xc), `collisionObject` (+0x18, an `ICollisionObject*`), `hasContact` (+0x20). |
| `RayCastResult` | 0x30 | What `Track::rayCast` returns: `surfaceDef` (+0x0), `pos` (+0x8), `normal` (+0x14), `hasHit` (+0x20), `collisionObject` (+0x28). |
| `SurfaceDef` | 0xc8 | One surface type: sound name, grip, dirt, bumps, validity, pit lane flag, damping, steering vibration. Full table in section 4. One heap copy per collision mesh. |
| `ICollisionObject` → `CollisionMeshODE` | 0x30 | One static ODE triangle mesh: `trimeshData`, `trimesh`, private copies of the vertices (`lvertices`) and 16-bit indices (`lindices`), and `userPointer` (+0x28), which holds the mesh's `SurfaceDef*`. Slots: +0x00 `release`, +0x08 `setUserPointer`, +0x10 `getUserPointer`, +0x18 `getGroup` (category bits), +0x20 `getMask` (collide bits), +0x28 destructor. |
| `PhysicsCore` | 0xa8 | The ODE wrapper (see `docs/map/physics_engine.md`). Relevant members: `spaceStatic` (+0x10), `ray` (+0x50, the shared ray), `staticSubSpaces` (+0x68, `std::map<unsigned, dxSpace*>`). |
| `RayContactAccumulator` | 0x48 | Scratch struct used during one ray cast: `depth` (+0x0, starts at −1) and the best `dContactGeom` so far (+0x8). |
| `DynamicTrackData` | 0x18 | `isExternal`, `enabled`, `sessionStartGrip`, `baseGrip`, `randomGrip`, `gripPerLap`, `sessionTransfer`. Member `Track::dynamicTrack` (+0xb0). |
| `SurfacesManager` | 0x20 | Graphics-side loader: `std::map<wstring, SurfaceDef> surfaces` and `enableCrash`. Member of `TrackAvatar` (+0x250). |
| `TrackAvatar` | 0x308 | Graphics-side track object. Loads the kn5 models, builds the physics meshes, timing lines, spawn points. Owns the physics `Track`. |
| `NKUtils` | 0x8 (vtable only, no data) | Helper with one method, `getSectorID` (leading integer of a mesh name). |
| `TrackPhysicsStats` | 0x18 | Counters filled while loading: `objects`, `tris`, `groups` (meshes per number prefix). |
| `DynamicTrackManager` | 0x90 | Graphics only: fades the rubbered-in "groove" meshes in and out. Reads `dynamicGripLevel`, never writes physics. |
| `DynamicTrackStatus` | 0x8 | `enabled`, `currentGripLevel`: the copy of the grip level that `PhysicsAvatar` hands to the render thread. |
| `TyreTester` | 0x128 | The game's own fake flat ground (it is both an `ISuspension` and an `IRayTrackCollisionProvider`). Useful as a model for an oracle, see section 9. |

Not physics, listed only: `TrackObject` / `PhysicsObject` (movable objects named `AC_POBJECT…`,
they are rigid bodies and belong to the body/collision map), `TrackAudio`, `TrackMap`,
`StartingTrackSemaphore`, `TrackAvatar::SectorDescription` (corner names from `sections.ini`).

---

## 3. Key functions

### 3.1 Loading / init

```
Sim::loadTrack                                             0x14019a4c0
└─ TrackAvatar::TrackAvatar                                0x1401c5250
   ├─ Track::Track(engine, name, config)                   0x140277100   physicsTrack; sets engine->track
   │  ├─ Track::initDynamicTrack                           0x140278300   cfg/race.ini [DYNAMIC_TRACK]
   │  │  └─ Event<SessionInfo>::addHandler(engine->evOnNewSessionPhysics, lambda 0x140277740)
   │  └─ DRSManager::DRSManager                            0x140278ea0   data/drs_zones.ini (aero map)
   ├─ SurfacesManager::SurfacesManager                     0x1401ae9b0
   │  ├─ built-in "WALL" surface (category 2)
   │  ├─ system/cfg/assetto_corsa.ini [ASSETTO_CORSA] WARNINGS_AS_ERRORS → enableCrash
   │  ├─ SurfacesManager::loadSurfaceDefinitions("system/data/surfaces.ini")           0x1401afad0
   │  └─ SurfacesManager::loadSurfaceDefinitions("content/tracks/<track>[/<config>]/data/surfaces.ini")
   ├─ TrackAvatar::init3D                                  0x1401c8740   <track>.kn5 or models[_<config>].ini
   ├─ TrackAvatar::initPhysics                             0x1401ca440
   │  ├─ TrackAvatar::processPhysicsNode(model root)       0x1401cc5e0   recursive over all nodes
   │  │  ├─ NKUtils::getSectorID(mesh name)                0x14018d2b0   leading integer; 0 = not physical
   │  │  └─ TrackAvatar::addPhysicsMesh(mesh, id)          0x1401c78e0
   │  │     ├─ TrackAvatar::getSurfaceDescFromMeshName     0x1401c8300
   │  │     │  └─ SurfacesManager::getSurface              0x1401af340   substring match on KEY
   │  │     └─ Track::addSurface(name, verts, nVerts, indices, nIndices, surfaceDef, spaceId)   0x140277e50
   │  │        ├─ core->createCollisionMesh  (IPhysicsCore +0x38) → PhysicsCore::createCollisionMesh 0x1402cc0f0
   │  │        │  └─ CollisionMeshODE::CollisionMeshODE    0x1402cebb0
   │  │        │     ├─ dGeomTriMeshDataBuildSingle (12-byte vertices, 6-byte triangles = 16-bit indices)
   │  │        │     ├─ PhysicsCore::getStaticSubSpace(id) 0x1402ccac0   one dSimpleSpace per id
   │  │        │     └─ dCreateTriMesh, dGeomSetData(this), dGeomSetCategoryBits, dGeomSetCollideBits
   │  │        ├─ new SurfaceDef (0xc8 bytes), copy of the argument
   │  │        └─ mesh->setUserPointer(copy) (ICollisionObject +0x08) → CollisionMeshODE::setUserPointer 0x1402cedb0
   │  └─ Track::initAISpline                               0x1402782a0   AISplineRecorder, then
   │     └─ Track::initStartingBounds                      0x140278790   data/starting_bounds.ini
   ├─ TrackAvatar::initDynamicObjects                      0x1401c9840   models ini [DYNAMIC_OBJECT_n] (graphics only)
   ├─ TrackAvatar::initRespawnPositionSet                  0x1401cb8c0   spawn nodes AC_PIT_n / AC_START_n / AC_TIME_ATTACK_n
   ├─ TrackAvatar::initTimeLines                           0x1401cbc90   nodes AC_TIME_n_L/_R, AC_AB_…, AC_OPEN_…
   │  └─ Track::addTimeLine                                0x140278040   also fills sectorsNormalizedPositions / isOpen
   ├─ DynamicTrackManager::DynamicTrackManager             0x1401ccc20   data/groove.ini (graphics)
   └─ TrackObject::TrackObject (nodes "AC_POBJECT…")       0x1401cf1a0   → PhysicsObject::PhysicsObject 0x1402ac8a0

Car::Car                                                   0x14026bf00
└─ Tyre::init(hub, engine->track, …)                       0x140280650   per wheel
   └─ provider->createRayCaster(3.0) (+0x18) → Track::createRayCaster 0x1402781f0
      └─ core->createRayCaster (IPhysicsCore +0x88) → PhysicsCore::createRayCaster 0x1402cc360
         └─ RayCaster::RayCaster                           0x1402cedc0   dCreateRay(0, 3.0), first contact 1, backface cull 1
```

Two small `Track` members that are filled here and are not ground contact (confirmed, listed for
completeness): `Track::addTimeLine` pushes a `TimeLine`; for a line of type AB start/finish it sets
`isOpen` (+0x88) instead, otherwise it projects the midpoint of the gate onto the best-lap AI line
(`InterpolatingSpline::worldToSpline`) and appends that position to `sectorsNormalizedPositions`
(+0x130). `Track::getSector(pos)` 0x140278220 returns the index of the interval of that list that
contains a normalised spline position (last index when none does); its only caller is
`evaluateTimeFromTrackSpline` (in `TimeAttack.obj`). `Track::~Track` 0x140277500 calls `release()` on every
entry of `surfaces`.

### 3.2 Every physics step

```
PhysicsEngine::step(dt)                                    0x140264760
├─ Track::step(dt)                                         0x140278d20   before any car
│  ├─ dynamicGripLevel update (section 5.1)
│  ├─ drsMamanger step: call to 0x140017870, the empty function (nothing happens)
│  └─ AISplineRecorder::step                               0x140296f60   only when aiSplineRecorder exists
├─ … Car::step (each car) → Car::stepComponents → Tyre::step 0x140283800 (each wheel)
│  └─ rayCaster->rayCast(org, dir) (IRayCaster +0x08) → RayCaster::rayCast 0x1402cee90
│     └─ PhysicsCore::rayCast(org, dir, dxGeom* ray)       0x1402cd070
│        ├─ dGeomRaySet(ray, org, dir)
│        ├─ dSpaceCollide2(ray, spaceStatic, &accumulator, rayNearCallback)
│        │  └─ rayNearCallback                             0x1402cd210   recurses into sub-spaces; dCollide(…, 1 contact)
│        │     └─ dCollideRTL (ODE ray-trimesh, OPCODE)    0x14038aa90
│        └─ dGeomGetData(contact.g2) → CollisionMeshODE*
│     then in Tyre::step: hit.collisionObject->getUserPointer() (+0x10) → SurfaceDef*
│  (fallback when the tyre has no ray caster: provider->rayCast(org, dir, &result, 2.0) (+0x08)
│   → Track::rayCast 0x140278bb0 → core->rayCast (IPhysicsCore +0x40) → PhysicsCore::rayCast 0x1402cd1a0,
│   which sets the length of the shared ray and calls the function above)
└─ core->step: PhysicsCore::collisionStep → nearCallback   0x1402ccc70   car boxes ↔ ground, car mesh ↔ walls
```

On a session change (not every step): the `PhysicsAvatar` lambda at 0x1401213f0 calls
`PhysicsEngine::setSessionInfo` 0x140264560, then runs every `evOnNewSessionPhysics` handler,
among them the `Track` lambda at 0x140277740 (new `baseGrip`, section 5.1), then asks the core for
250 collision-free steps.
Two more things happen on a new session, both queued onto the physics thread by graphics-side
handlers (`PhysicsAvatar::executeOnPhysicsThread`): `CarAvatar::onNewSession` 0x1400d91c0 queues a
lambda that calls `TimeTransponder::reset` 0x140290e60 (`lapCount = 0`, the input of
`Track::step`), and the `TrackAvatar` lambda at 0x1401c6b70 queues one that calls
`TrackObject::resetOrgMatrix` 0x1401cf6d0 on the movable track objects.
In multiplayer `Track::setGripLevelExternal` 0x140278d00 is called from
`ACClient::onRemoteLapCompleted` 0x14004f470 and from the ACClient lambda at 0x14003cf40.

`Track::rayCastWithRayCaster` 0x140278c80 exists (slot +0x10) but the tyre does not use it: with a
ray caster the tyre calls the caster directly and fetches the `SurfaceDef` itself.

Other users of `Track::rayCast` (slot +0x08, shared ray), found by searching for the call pattern
on an `IRayTrackCollisionProvider*`:

- **Per physics step, online only:** `NetCarStateProvider::step` 0x14011bd00 (one per remote car,
  registered as an `evOnStepCompleted` handler in its constructor; `Sim::addNetCar` 0x140197c30
  hands it `&track->physicsTrack` as `rayCastProvider`, member +0x6b8). It casts one ray of length
  10 from 1 m above the received car position (to keep `targetHeight`), and one ray of length 5
  from 1 m above each wheel position, and copies the hit `SurfaceDef` into the remote car's
  `state.tyreSurfaceDef[i]`. `stepLagging` and `getSmoothBodyMatrix` do the same with length 10.
  So the shared ray is not idle during a multiplayer step; offline nothing uses it per step.
- At load / AI-line save: `AISpline::initPitlane` 0x1402a7f70 (one ray of length 100 from 2 m
  above every point of the pit-lane AI line; stores the surface's `gripMod` and `isPitlane` in the
  AI line payload), `AISpline::buildSides` 0x1402a5630 (walks sideways from the AI line with rays
  of length 100 until the hit surface has `isValidTrack == false`: the track-edge distances of the
  AI line), `AISpline::calculateNormals` 0x1402a5e10 (rays under the AI line; reads `gripMod`).
- Graphics: `IdealLine::IdealLine` 0x140109d10 and the `LollipopCrew` constructor 0x140112bf0
  (put the ideal line and the pit crew on the ground).

---

## 4. Data it reads

### 4.1 `surfaces.ini` → `SurfaceDef`

Two files are read in this order by `SurfacesManager::SurfacesManager`; the second overrides
entries with the same `KEY` (it is a `std::map` assignment):

1. `system/data/surfaces.ini` (4 entries: `ROAD`, `GRASS`, `KERB`, `SAND`).
2. `content/tracks/<track>/data/surfaces.ini`, or `content/tracks/<track>/<config>/data/surfaces.ini`
   when the track has a layout. A missing file only prints
   `WARNING: Surface definition file %S not found`.

Sections are `[SURFACE_0]`, `[SURFACE_1]`, … and reading **stops at the first missing number**.
A missing key gives 0 / empty (`INIReader::getFloat` 0x1402358c0 returns 0.0 for an absent or empty
value). No version switch.

| Key | `SurfaceDef` member (offset) | Conversion | Used by |
|---|---|---|---|
| `KEY` | map key in `SurfacesManager::surfaces` (not stored in the struct) | text, stored exactly as written (the loader does not upper-case it) | Matched as a **substring** of the upper-cased mesh name (section 5.3). The comparison is case-sensitive, so a `KEY` with lower-case letters can never match. |
| `FRICTION` | `gripMod` (+0x90) | float | Tyre grip multiplier (`Tyre::addTyreForcesV10`, `addTyreForces`), tyre heating (`stepThermalModel`), graining (`stepGrainBlister`), flat spots (`stepFlatSpot`); AI line payload (`AISpline::initPitlane`, `calculateNormals`); graphics: skid marks, smoke. |
| `DAMPING` | `damping` (+0xb4) | float, 1/s | `Tyre::step`: extra drag on the whole car body while a wheel is on the surface (section 5.5). |
| `WAV` | `wavString` (+0x00, `wchar_t[64]`) | text, copied without a length check | Audio only (`CarAudioFMOD::updateSurfaces`, `updateSkids`). |
| `WAV_PITCH` | `wavPitchSpeed` (+0x80) | float | Audio only. |
| `FF_EFFECT` | **not read** | – | Present in every shipped file (`NULL`, `0`, `1`) but no code looks for the string. |
| `DIRT_ADDITIVE` | `dirtAdditiveK` (+0x98) | float | `Tyre::stepDirtyLevel`: how fast the tyre picks up dirt; 0 means the tyre cleans itself. Graphics: dust, drift mode. |
| `IS_VALID_TRACK` | `isValidTrack` (+0xa0) | int ≠ 0 | `LapInvalidator::step` (tyres-out count), `TimeAttack::updatePenalties`, `AISpline::buildSides` (where the track edge is for the AI line). |
| `BLACK_FLAG_TIME` | `blackFlagTime` (+0xa4) | float, seconds | `LapInvalidator::step`: largest value among the wheels that are off track becomes the penalty time. |
| `SIN_HEIGHT` | `sinHeight` (+0xa8) | float, metres | `Tyre::step`: depth of the procedural bumps (section 5.4). |
| `SIN_LENGTH` | `sinLength` (+0xac) | float, rad/m | `Tyre::step`: spatial frequency of the bumps. It is used as `sin(SIN_LENGTH · x)`, so it is not a length; wavelength = 2π / value. |
| `IS_PITLANE` | `isPitlane` (+0xb0) | int ≠ 0 | `SpeedLimiter::step`, `Car::isInPitLane`, `Car::updateColliderStatus`, `Car::getEngagement`, `AISpline::initPitlane` (AI pit line, at load), pit crew graphics. |
| `VIBRATION_GAIN` | `vibrationGain` (+0xbc) | float | `Car::pollControls`: strength of the kerb vibration sent to the wheel (section 5.6). |
| `VIBRATION_LENGTH` | `vibrationLength` (+0xc0) | float, metres | `Car::pollControls`: period of that vibration in metres travelled. |
| – | `collisionCategory` (+0x9c) | hard-coded **1** for every ini entry; 2 for the built-in `WALL`; 0 for "no match" | ODE category bits of the mesh (section 5.7). |
| – | `granularity` (+0xb8) | hard-coded **0** (no key is read into it) | Would switch on a three-sine bump pattern in `Tyre::step` and a loose-surface formula in `Tyre::addTyreForces`; never non-zero in this build. |
| – | `sectorID` (+0x94) | leading integer of the mesh name (set in `getSurfaceDescFromMeshName`) | Only copied around (replay, network, shared state). |
| – | `userPointer` (+0x88) | the graphics `Mesh*` (set in `addPhysicsMesh`) | Graphics side. |

Built-in defaults (not from any file):

| Surface | Where | Values |
|---|---|---|
| `WALL` | `SurfacesManager::SurfacesManager` | `gripMod` 1.0, `collisionCategory` 2, `isValidTrack` true, `vibrationLength` 1.5, everything else 0, empty `wavString`. A track file may replace it by defining `KEY=WALL` (then it gets category 1). |
| "no match / ambiguous" | `SurfacesManager::getSurface` | `gripMod` 1.0, `collisionCategory` 0, `isValidTrack` true, `vibrationLength` 1.5, everything else 0. |

Values in the shipped files (48 files, 454 `[SURFACE_n]` sections, survey script in
`re/scratch/track_surface/`; a few layout folders in this install are third-party additions and
carry extra keys such as `ALLOW_APPS` that the game ignores):

| Key | Range seen | Most common |
|---|---|---|
| `FRICTION` | 0.6 … 0.99 (system `ROAD` = 1) | 0.95, 0.96, 0.6 (grass), 0.99 |
| `DAMPING` | 0 … 0.15 | 0 (401 of 454), 0.01 |
| `DIRT_ADDITIVE` | 0 … 1 | 0, 1 (grass/sand), 0.1 |
| `IS_VALID_TRACK` | 0 / 1 | 1 (321), 0 (133) |
| `BLACK_FLAG_TIME` | 0 … 20 | 0 (362), 10, 5 |
| `SIN_HEIGHT` / `SIN_LENGTH` | 0 … 0.05 / 0 … 0.5 | 0 (409); grass 0.03 / 0.5 |
| `IS_PITLANE` | 0 / 1 | 0 (397), 1 (57) |
| `VIBRATION_GAIN` / `VIBRATION_LENGTH` | 0 … 1.0 / 0 … 3 | 0; kerbs 0.2–1.0 / 0.3–1.5 |

No file has a gap in its `SURFACE_n` numbering.

### 4.2 `cfg/race.ini` `[DYNAMIC_TRACK]` → `Track::dynamicTrack`

Read by `Track::initDynamicTrack` through `INIReaderDocuments` 0x140238350 (going by the class
name, the copy of `race.ini` in the user's documents folder; that constructor was not read. The
game folder has a commented template in `cfg/race.ini`).
If the section is missing, the dynamic track stays off and grip is 1.0.

| Key | Member | Conversion | Default (constructor) | Template value |
|---|---|---|---|---|
| section present | `enabled` | true | false | present |
| `LAP_GAIN` | `gripPerLap` | `0.01 / value` if value > 0, else 0 ("laps needed for +1 % grip") | 0.1 | 1 |
| `SESSION_START` | `sessionStartGrip` | `value · 0.01`, then raised to at least 0.85 | 1.0 | 100 |
| `RANDOMNESS` | `randomGrip` | `value · 0.01` | 0.01 | 0 |
| `SESSION_TRANSFER` | `sessionTransfer` | `value · 0.01` | 0.0 | 50 |
| – | `baseGrip` | `(rand() · 3.0518509e-05 · 2 − 1) · randomGrip + sessionStartGrip` (the constant is 1/32767) | 1.0 | |
| – | `isExternal` | set by `setGripLevelExternal` | false | |

### 4.3 Other files touched by `Track`

| File | Read by | Purpose |
|---|---|---|
| `content/tracks/<track>[/<config>]/data/starting_bounds.ini` (`[BOUND_n] MIN, MAX`) | `Track::initStartingBounds` | Fills `Track::startingBounds` (spline index ranges), read by `SplineLocator::step`. Not ground contact; purpose not traced further. |
| `content/tracks/<track>[/<config>]/data/drs_zones.ini` (`[ZONE_n] DETECTION, START, END`) | `DRSManager::DRSManager` | DRS zones, read by `DRS::step` (aero map). |
| `content/tracks/<track>/<track>.kn5` or `models.ini` / `models_<config>.ini` (`[MODEL_n] FILE, POSITION, ROTATION`) | `TrackAvatar::init3D` | Which kn5 files make up the track. A missing `models_<config>.ini` is a fatal error. |
| `system/cfg/assetto_corsa.ini` `[ASSETTO_CORSA] WARNINGS_AS_ERRORS` | `SurfacesManager::SurfacesManager` | When non-zero, an unmatched or ambiguous mesh name crashes on purpose (`ksGenerateCrash`). Shipped value 0. |

Car data: none. This system reads no file from `cardata/`.

---

## 5. Each step: inputs, maths, outputs

### 5.1 `Track::step` — the track-wide grip level

Confirmed from the disassembly of 0x140278d20 (this settles the open question about the clamp in
`docs/map/car_step.md`).

```
if dynamicTrack.enabled:
    laps = Σ over engine->cars of (int) car->transponder.lapCount        (Car +0x204)
    v    = (float)laps · dynamicTrack.gripPerLap + dynamicTrack.baseGrip
    dynamicGripLevel = 1.0   if v > 1.0
                       0.85  if v < 0.85 (also when v is NaN)
                       v     otherwise
else if not dynamicTrack.isExternal:
    dynamicGripLevel = 1.0
(else: external value is left alone)
```

So it is a plain clamp to [0.85, 1.0]. The lap sum is recomputed from scratch every step; nothing
is integrated, and there is no per-surface or per-position rubber: one number for the whole track.

New session (lambda 0x140277740, only when `enabled` and not `isExternal`):

```
r = (float)rand() · 3.0518509e-05 · 2.0 − 1.0
session index 0:  baseGrip = r · randomGrip + sessionStartGrip
otherwise:        baseGrip = r · randomGrip + ((dynamicGripLevel − sessionStartGrip) · sessionTransfer + sessionStartGrip)
```

The bracket is computed before `rand()` is called. `Track::initDynamicTrack` already runs the
"session index 0" formula once at load (one `rand()` call there too), so `baseGrip` is valid before
the first session event. The lap counters are reset on a new session by `TimeTransponder::reset`
0x140290e60 (queued by `CarAvatar::onNewSession`, see 3.2), so the grip gained so far survives only
through `sessionTransfer`. The lambda reads the stored `dynamicGripLevel`, not the lap counts; which
of the two queued actions runs first, and whether a `Track::step` can fall between them, was not
traced (open question 8).

`Track::setGripLevelExternal(g)`: `dynamicGripLevel = g`, `isExternal = true`, `enabled = false`.
After that `Track::step` never touches the value again (online, the server owns it).

Reads: `engine->cars`, `Car::transponder.lapCount`, `dynamicTrack.*`. Writes: `dynamicGripLevel`
(+0x128), `dynamicTrack.baseGrip`.

### 5.2 The ground ray, end to end

Who owns the ray: each `Tyre` owns one `RayCaster` (created in `Tyre::init` with length **3.0 m**,
released in the tyre's destructor through slot +0x10). The `PhysicsCore` also owns one shared ray
(created with length 100) that serves `Track::rayCast`; its length is set on every call.

Per wheel, per step (`Tyre::step`, confirmed in the disassembly):

1. `origin = worldPosition + (0, 2.0, 0)`, `direction = (0, −1, 0)`. World Y is up.
   `worldPosition` is the translation of the hub matrix.
2. With a caster: `hit = rayCaster->rayCast(origin, direction)`; ray length 3.0, so the ground is
   found from 2 m above the wheel centre down to 1 m below it.
   Without a caster (only happens when `createRayCaster` returned null, e.g. `TyreTester`):
   `provider->rayCast(origin, direction, &result, 2.0)`; length 2.0, so only down to the wheel
   centre height.
3. `PhysicsCore::rayCast(org, dir, ray)`: zero the result, accumulator depth = −1,
   `dGeomRaySet`, then `dSpaceCollide2(ray, spaceStatic, &acc, rayNearCallback)`.
   Only the **static** space is searched: cars and movable objects are never hit.
4. `rayNearCallback(acc, g1, g2)`: if either geom is a space, recurse with `dSpaceCollide2`
   (this is how the numbered sub-spaces are entered). Otherwise, if either geom has a body, ignore
   it. Otherwise `n = dCollide(g1, g2, 1, contacts, 0x40)` (at most one contact) and keep the
   contact when `acc.depth < 0` or `contact.depth < acc.depth` (strictly nearer; a tie keeps the
   first one found). `depth` of a ray contact is the distance along the ray.
5. Back in `rayCast`: if `acc.depth >= 0` → `hasContact = true`, `pos = contact.pos`,
   `normal = contact.normal`, `collisionObject = dGeomGetData(contact.g2)` (the `CollisionMeshODE`
   that owns the triangle mesh).
6. The tyre (or `Track::rayCast`) calls `collisionObject->getUserPointer()` (slot +0x10) and gets
   the `SurfaceDef*` that `Track::addSurface` stored there. This is the whole "triangle →
   surface" mapping: **one surface per mesh, not per triangle.**

Ray settings (both the per-tyre rays and the shared one): `dGeomRaySetFirstContact(ray, 1)`,
`dGeomRaySetBackfaceCull(ray, 1)`. `dGeomRaySetClosestHit` is not linked at all, so closest-hit
mode is off. `dCollideRTL` reads all three settings and runs `Opcode::RayCollider::Collide`
(confirmed from its call list). Meaning (ODE source): inside one mesh the collider stops at the
**first** front-facing triangle its tree walk finds, not necessarily the nearest; between meshes
the nearest wins (step 4). Triangles that face away from the ray (seen from behind) are skipped.

Category bits play no role for rays in Kunos' code: `rayNearCallback` has no bit test, and the
rays keep ODE's default bits (all ones), so ODE's own pre-filter lets every mesh through (ODE
source). **The wheel ray therefore also hits walls and unmatched meshes** if they are below the
wheel; it then gets the `WALL` or the "no match" surface (grip 1.0).

### 5.3 From kn5 mesh to physics mesh

`TrackAvatar::processPhysicsNode(node)`, called on the model root and recursively on every child:

1. If the node is a `Mesh`: `id = NKUtils::getSectorID(name)` = the integer at the start of the
   name (`wcstol`, base 10). A name that does not start with a number, or whose leading number is
   0, gives 0 and the mesh is **not physical**. (For a non-numeric name `wcstol` consumes nothing and the code
   throws `invalid stoi argument`; the function has a catch block and the result is 0.)
2. If `id != 0`: statistics are counted (`physicsStats.objects`, `.tris`, `.groups[id]`) and
   `addPhysicsMesh(mesh, id)` runs.
3. Independently, if the **node** name starts with `AC_` the node is hidden (`isActive = false`).
   Those are helper nodes (timing gates, spawn points, pit boxes).
4. Recurse into `node->nodes`.

`TrackAvatar::addPhysicsMesh(mesh, id)`:

1. Copy the position (first 12 bytes of each 44-byte `MeshVertex`) of every vertex into a temporary
   `vec3f` array. **No matrix is applied**: the positions are used as stored in `Mesh::vertices`.
   A mesh with no vertices is skipped. `mesh->isStatic = true`.
2. `surfaceDef = getSurfaceDescFromMeshName(name)`:
   upper-case the name; `SurfacesManager::getSurface` collects every entry whose `KEY` occurs
   **anywhere** in the name (`wstring::find`); exactly one match → that entry; zero matches →
   prints `ERROR: SURFACE NOT FOUND FOR OBJECT:%S`, more than one → prints
   `ERROR: Mesh <name> CAN BE: <keys>`; both error cases return the "no match" default (and crash
   when `enableCrash`). Then `sectorID` = leading integer of the name.
3. If `surfaceDef.collisionCategory == 2` (the built-in `WALL`): `id += 10000`. Walls live in their
   own sub-spaces.
4. `surfaceDef.userPointer = mesh`.
5. `Track::addSurface(name, verts, nVerts, mesh->indices, nIndices, surfaceDef, id)`.

`Track::addSurface`:
`core->createCollisionMesh(verts, nVerts, indices, nIndices, &worldMatrix, body = 0,
category = surfaceDef.collisionCategory, mask = 0x14, spaceId = id)`.
`PhysicsCore::createCollisionMesh` **drops the matrix and the body** and only forwards the rest.
`CollisionMeshODE` copies vertices and indices into its own arrays, builds the ODE mesh data
(vertex stride 12, triangle stride 6), creates the triangle mesh inside
`PhysicsCore::getStaticSubSpace(id)` and stores itself as the geom's user data. Then a heap copy of
the `SurfaceDef` is attached with `setUserPointer` and the mesh is appended to `Track::surfaces`.

Sub-spaces: `getStaticSubSpace(0)` is `spaceStatic` itself; any other id gets its own
`dSimpleSpace` created inside `spaceStatic` on first use (prints
`CREATING STATIC SUBSPACE AT INDEX:%d`) and kept in the map `staticSubSpaces`. My reading of the
purpose: the number prefix lets the track builder group meshes that are close together, so a ray
or a car body is first tested against the bounding box of the whole group and skips far-away
groups cheaply. It has no effect on which surface a mesh gets.

Name convention in one line: `<group number><anything containing exactly one surface KEY>`, for
example `1ROAD`, `1KERB_L3`, `12GRASS_07`, `3WALL_pit`. `1ROADKERB` is ambiguous (two keys match)
and becomes a "no match" surface.

16-bit limit: `Mesh::indices` is a `std::vector<unsigned short>` and ODE is built with 16-bit
triangle indices, so one mesh addresses at most 65 536 vertices. **There is no splitting code in
this path**: one kn5 mesh becomes exactly one ODE triangle mesh. Whatever keeps meshes under the
limit happens before the game (the kn5 format / exporter).

### 5.4 What the tyre does with the hit (interface check)

Read from `Tyre::step`; detailed tyre maths is in `docs/map/tyre.md`.

- The hit is used only if the wheel's up axis points up: `worldRotation.M22 > 0.35`. Otherwise the
  wheel is treated as airborne (`status.ndSlip = 0`, `status.Fy = 0`, `surfaceDef` stays null).
- `Tyre::surfaceDef` (+0x410) is set to null at the start of every step and to the hit's surface
  when the hit is accepted. Everything else in the car that asks "what is this wheel standing on"
  reads this pointer after the tyre step.
- `unmodifiedContactPoint = hit.pos`.
- `c = hit.normal · wheelUp` (`wheelUp` = row 2 of the hub rotation).
  - `c > 0.96`: `contactPoint = wheelPos + normal · (normal · (hit.pos − wheelPos))`
    (wheel centre projected onto the hit plane).
  - else: `contactPoint = hit.pos` and the normal is rotated about `normal × wheelUp` by
    `acos(c) − acos(0.96)` (`mat44f::createFromAxisAngle` 0x1400571a0), which leaves it exactly
    `acos(0.96)` ≈ 16.3° away from the wheel's up axis. `acos(c)` is replaced by 0 when
    `c <= −1` or `c >= 1`.
- Bumps: if `sinHeight != 0`:
  `contactPoint.y −= (sin(sinLength · contactPoint.x) · cos(sinLength · contactPoint.z) + 1) · sinHeight`.
  The ground is only ever lowered, by 0 to `2 · sinHeight`, as a fixed pattern in world X/Z.
- Dead in this build (`granularity` is always 0): for (f, a) in ((1.0, 0.005), (5.8, 0.005),
  (11.4, 0.01)): `contactPoint.y += (sin(f·x) · cos(f·z) + 1) · a · (−0.6)`.
- `contactNormal` = the (possibly rotated) normal, then `Tyre::addGroundContact` 0x14027d980 and the
  force function get `contactPoint`, the normal and the `SurfaceDef*`.
- Grip: `D input = getCorrectedD(…) · surfaceDef->gripMod · track->dynamicGripLevel` (V10 path); the
  old path multiplies `Fx` and `Fy` by `dynamicGripLevel · gripMod` at the end. The track pointer
  is reached as `tyre->car->ksPhysics->track`; with a null `car` the factor is 1.0.
- Heating: `thermalInput = slideSpeed · D · load · thermalFrictionK · dynamicGripLevel · gripMod`.
- Dirt (`Tyre::stepDirtyLevel(dt, speed)` 0x1402843d0, called from both force functions with
  `speed = |angularVelocity · effectiveRadius|`, the wheel's rolling speed, not the car speed):
  while `dirtyLevel < 5`: `dirtyLevel += speed · dirtAdditiveK · 0.03 · dt`; when
  `dirtAdditiveK == 0` and `dirtyLevel > 0`: `dirtyLevel −= speed · 0.015 · dt`, floored at 0.
  The grip loss from `dirtyLevel` is tyre maths.

### 5.5 Surface damping (sand, gravel, "OUT" strips)

In `Tyre::step`, after the tyre forces, once per wheel that stands on a surface with
`damping > 0`:

```
v = car->body->getVelocity()                (IRigidBody +0x78)
m = car->body->getMass()                    (IRigidBody +0x28)
F = (−damping · v.x · m, −damping · v.y · m, −damping · v.z · m)
car->body->addForceAtLocalPos(F, (0, 0, 0)) (IRigidBody +0xf8)
```

It is a drag on the **car body**, proportional to body mass and velocity, applied at the body
origin, and it adds up over the wheels: four wheels in sand with `DAMPING = 0.1` decelerate the car
at roughly `0.4 · v` m/s² (the force uses the body's mass, not the whole car's). The pseudo-C
dereferences `tyre->car` here without a null check.

### 5.6 Kerb vibration for the steering wheel

In `Car::pollControls` 0x140274e70 (runs inside `Car::step`, before the components):

```
vibrationPhase += speed · dt                                   (metres travelled)
gain   = max over the 4 tyres of surfaceDef->vibrationGain     (0 if no surface)
length = mean of surfaceDef->vibrationLength over the tyres whose length != 0 and gain != 0
if gain != 0 and length != 0:
    kerb = ksSawToothWave(vibrationPhase, length) · sat(tyres[0].status.load)
           · sat(tyres[1].status.load) · gain · sat(speed)
kerb = kerb · sat(speed)        (second multiply, outside the `if`: the kerb term carries sat(speed)²)
```

`sat(x)` = clamp to [0, 1]; the loads are in newtons, so the two factors only mean "both front
wheels are on the ground". `ksSawToothWave` is at 0x14022d2a0. `speed` is `Car::getSpeed()` in m/s.
The whole block only runs when the car has a `controlsProvider` and is neither `isControlsLocked`
nor `blackFlagged`; otherwise `vibrationPhase` does not advance either. When a tyre has a gain but
no tyre qualifies for the mean, `length` is 0/0 = NaN; the test is `ucomiss` + `je`, which treats
NaN as equal to 0, so the block is skipped. The value goes into the vibration
packet handed to the controls provider (force-feedback map). The surface data is one step old here,
because the tyres have not run yet in this step.

### 5.7 What collides with what (bodies, not wheels)

Category and collide bits found in the code:

| Geom | Category bits | Collide bits | Set in |
|---|---|---|---|
| Track mesh matched to a `surfaces.ini` key | 1 | 0x14 | `Track::addSurface` |
| Track mesh matched to the built-in `WALL` | 2 | 0x14 | same |
| Track mesh with no / ambiguous match | 0 | 0x14 | same |
| Car body shell (collider mesh) | 4 | 0x1e at creation; each step `Car::updateColliderStatus` 0x140276df0 sets 0x1e, or 0x1a while pit-lane ghosting is on, plus bit 0 when the car lies on its side or roof (`M22 < 0.25`) | `Car::initColliderMesh` 0x140273b20 → `RigidBodyODE::addMeshCollider` 0x1402ce080, `RigidBodyODE::setMeshCollideMask` 0x1402ce9c0 |
| Car underside boxes (`colliders.ini`) | 4 | 1 | `CarColliderManager::loadINI` 0x1402a37a0 → `RigidBodyODE::addBoxCollider` 0x1402cddb0 |
| Movable track object | 0x10 | 0x1f | `PhysicsObject::PhysicsObject` |
| Tyre rays, shared ray | ODE default | ODE default | never set |

`nearCallback` requires a match **in both directions**: `(catA & maskB) != 0` and
`(catB & maskA) != 0`. Resulting pairs:

| | Ground surface (cat 1) | Wall (cat 2) | Unmatched (cat 0) |
|---|---|---|---|
| Car underside boxes | **collide** | no | no |
| Car body shell | only when overturned | **collide** | no |
| Movable track object | collide | collide | no |
| Wheel ray | hit | hit | hit |

So bit 0 = ground, bit 1 = wall, bit 2 = car, bit 4 = movable object (bit 3 was not seen on any
category). Contact friction for bodies does **not** come from `SurfaceDef`:
`PhysicsCore::onCollision` uses fixed values (0.25 normally, 0.1 for a box on a triangle mesh; see
`docs/map/physics_engine.md` section 3.6). `FRICTION` in `surfaces.ini` only reaches the tyres.

---

## 6. Connections to other systems

### Who writes this system's inputs

| Input | Written by | When |
|---|---|---|
| `Track::surfaces`, the ODE meshes, the `SurfaceDef` copies | `TrackAvatar::addPhysicsMesh` → `Track::addSurface` | Track load only. Never changed afterwards. |
| `Track::dynamicTrack.*` | `Track::Track` (defaults), `Track::initDynamicTrack` (race.ini), session lambda | Load, each new session. |
| `Car::transponder.lapCount` | `TimeTransponder::step` (last-but-one entry of `Car::stepComponents`) | When a car crosses the finish line. `Track::step` sees it on the next step. |
| `Track::dynamicGripLevel` from outside | `Track::setGripLevelExternal` (multiplayer client) | Server messages. |
| `PhysicsEngine::track` | `Track::Track` (deletes a previous track first) | Track load. |
| Wheel position and orientation for the ray | `ISuspension` slot +0x08 (hub world matrix), read inside `Tyre::step` | Every step. |

### Who reads this system's outputs

| Output | Read by | Where in the step |
|---|---|---|
| Ray hit (point, normal, `SurfaceDef*`) | `Tyre::step` (own ray caster). Online also `NetCarStateProvider::step` for every remote car (shared ray, section 3.2) | `Car::stepComponents`, the four `Tyre::step` calls (after `BrakeSystem::step`, `EDL::step` and the suspension steps). The remote-car rays run in the `evOnStepCompleted` handlers, after `core->step`. |
| `Track::dynamicGripLevel` | `Tyre::addTyreForcesV10`, `Tyre::addTyreForces`, `Tyre::stepThermalModel`; `RaceEngineer::evalAvailableBrake` and `evalLateralGFromLoads` (AI/electronics estimates); `PhysicsAvatar::onPhysicsStepCompleted` 0x1401221a0 (copy for the render thread); shared memory writer; `DynamicTrackManager::update` 0x1401cdba0 (groove visuals) | Tyre step; step-completed handlers. |
| `Tyre::surfaceDef` → `gripMod` | `Tyre::stepFlatSpot`, `Tyre::stepGrainBlister` (both need `gripMod >= 0.95`, i.e. only on proper tarmac), `Tyre::stepThermalModel` | End of `Tyre::step`. |
| `Tyre::surfaceDef` → `dirtAdditiveK` | `Tyre::stepDirtyLevel` | Inside the tyre force function. |
| `Tyre::surfaceDef` → `damping` | `Tyre::step` → `Car::body` force | Tyre step. |
| `Tyre::surfaceDef` → `isPitlane` | `SpeedLimiter::step` 0x1402bb910 (`shoudLimit` = any wheel on a pit-lane surface; the code then compares speed · 3.6 with 80 and zeroes the engine's `electronicOverride`; details belong to the electronics map), `Car::isInPitLane` 0x1402744e0, `Car::updateColliderStatus` (car-to-car collisions off in the pits), `Car::getEngagement` | `SpeedLimiter::step` is later in the same `stepComponents`, so it sees this step's surfaces. |
| `Tyre::surfaceDef` → `isValidTrack`, `blackFlagTime` | `LapInvalidator::step` 0x1402c0580 (player car only): counts wheels on invalid surfaces, compares with `engine->allowedTyresOut`, takes the largest `blackFlagTime` | Later in the same `stepComponents`. |
| `Tyre::surfaceDef` → `vibrationGain`, `vibrationLength` | `Car::pollControls` | Start of the next `Car::step` (one step late). |
| Whole `SurfaceDef` per wheel | `Car::getPhysicsState` 0x140270d70 copies it into `CarPhysicsState::tyreSurfaceDef[4]` for audio (`wavString`, `wavPitchSpeed`), skid marks, smoke, dust, replay, network, time attack | After the step. |
| Static meshes in `spaceStatic` | `PhysicsCore::collisionStep` / `nearCallback` (car boxes on the ground, car shell on walls) | `core->step`, after all cars; car-vs-track pairs are rebuilt on odd steps only (physics_engine.md). |

Not owned by `Track` (asked in the brief): `ambientTemperature` (+0x100), `roadTemperature`
(+0x104) and `wind` (+0x158) are members of `PhysicsEngine`. Defaults 26 / 32 (30 after the second
constructor assignment), written by `RaceManager::initOffline` and by `ACClient::onMessageTCP`
online; wind is stepped by `PhysicsEngine::stepWind` 0x140265380 (see `docs/map/car_step.md`).
`roadTemperature` is read by the tyre thermal model, `ambientTemperature` by tyres, brakes, engine
cooling and air density. Pit boxes and grid positions are graphics-side spawn nodes
(`TrackAvatar::spawnPositions`, node names `AC_PIT_n`, `AC_START_n`, `AC_TIME_ATTACK_n`); physics
only receives a matrix when a car is teleported. The start-line and sector gates are
`Track::timeLines`, read by `TimeTransponder`.

---

## 7. Port notes

**Effort: S for everything except real track meshes; the mesh ray cast itself is L and belongs to
the physics-library port.**

- `SurfaceDef`, the `surfaces.ini` loader, the name matching rule, `Track::step`, the session
  lambda, the surface drag and the vibration formula are a few dozen lines each. **S.**
- The ray against a real track is ODE's ray-trimesh collider on top of OPCODE trees (stage 3 in
  `docs/map/physics_engine.md`). Bit-exact results need the same tree build and the same walk
  order, because first-contact mode returns the first triangle found. **L**, and not needed for a
  flat-ground car.
- Reading track `.kn5` files (to get the meshes at all) is a separate task outside physics.

Must exist first: nothing for the data side. The tyre port already defines the Rust trait
(`ray_cast` returning point, normal, surface; see `docs/port/tyre_step.md`). `Track::step` needs
only a lap counter per car.

Testing bit-exact:

| Piece | Oracle calls | What must be faked |
|---|---|---|
| `Track::step` clamp | `Track::step` 0x140278d20 on a hand-built 0x148-byte `Track`: set `ksPhysics` to a fake `PhysicsEngine` whose `cars` vector (+0x8 begin, +0x10 end) points at fake cars with a `lapCount` at +0x204; `aiSplineRecorder` (+0x80) = null; `drsMamanger` may be anything (the call target is the empty function) | Sweep `baseGrip`, `gripPerLap`, lap counts; include values below 0.85, above 1 and NaN. |
| Session lambda | Call 0x140277740 with a fake closure `{Track*}` and a `SessionInfo` whose `index` is at +0x1c | `rand()` is the CRT's: call `srand` with a fixed seed in the same process first and mirror the CRT generator in Rust. |
| `surfaces.ini` loading | `SurfacesManager::loadSurfaceDefinitions` 0x1401afad0 on a zeroed `SurfacesManager` with an empty map, then walk the map; or simply compare a Rust parser against the table in 4.1 | Needs `INIReader` to run (the tyre oracle already does that: `INIReader::useCache = 0`). |
| Name matching | `SurfacesManager::getSurface` 0x1401af340 with a list of names | Same object as above. |
| Ray against meshes | `PhysicsCore::rayCast` 0x1402cd070 on a real `PhysicsCore` (`ksPhysicsCoreODEFactory::create`), meshes added with `createCollisionMesh` | Needs ODE initialised (`dInitODE2`, thread data); this is the `ode_oracle` proposed in physics_engine.md. |
| Bumps, contact point, normal limit, surface drag | Already covered by running `Tyre::step` in the tyre oracle | Give the fake provider a surface with `sinHeight`/`sinLength`/`damping` set and a tilted normal; the drag needs a fake `car->body`. |

---

## 8. Open questions

1. **Physics vertices are not transformed.** `addPhysicsMesh` copies `MeshVertex::pos` as stored and
   `createCollisionMesh` ignores the matrix it is given. Confirmed in review: `TrackAvatar::init3D`
   turns the `models.ini` `POSITION`/`ROTATION` into the **node matrix** of the loaded model
   (`mat44f::createFromEuler`, `XMMatrixMultiply`, written to `Node::matrix` +0x08), it does not
   touch vertices; so a model entry with a non-zero position or rotation is drawn moved but
   collides unmoved, unless the kn5 loader stores world-space vertices. Still not checked: whether
   the kn5 loader (`KN5IO::load` 0x1402151a0, `Model::load` 0x140217d30) bakes the node transforms
   that are inside the kn5 file into the vertices. All shipped `models_*.ini` entries the author
   looked at use `0,0,0`.
2. **First-contact behaviour inside one mesh** (first triangle found, not nearest) and **ODE's
   pre-filter on category/collide bits for rays** are taken from the ODE 0.13.1 source. In the
   binary I only confirmed that Kunos sets first-contact and backface-cull to 1, never enables
   closest-hit, never sets bits on rays, and that `dCollideRTL` reads those settings. Whether a
   wheel can really "see" a wall top should be confirmed with the ODE oracle.
3. **Direction of the returned normal.** `dCollide` swaps the pair because the collider is
   registered for (mesh, ray) while the callback passes (ray, mesh); the tyre code clearly expects
   a normal pointing up out of the ground. The exact sign handling was reasoned from the ODE source,
   not traced in the binary.
4. **`granularity` is never non-zero** in this build as far as I can see (the loader writes a
   literal 0 and no other writer exists outside plain copies). So the three-sine pattern in
   `Tyre::step` and the loose-surface branch of `Tyre::addTyreForces` (which even writes
   `gripMod = 1.0` back into the shared `SurfaceDef`) look like dead code. A port can skip them, but
   I chose to keep them documented in case another build sets the field.
5. **`FF_EFFECT`** is in every surfaces.ini but no function references the string. I treat it as an
   ignored leftover.
6. **`WAV` longer than 63 characters** would overflow `wavString` in `loadSurfaceDefinitions`
   (unbounded copy). Irrelevant for a port, noted for completeness.
7. **`NKUtils::getSectorID` on non-numeric names**: the pseudo-C shows the throw and the PDB lists a
   catch funclet for the function; "returns 0" is my reading of the fall-through path, not a traced
   run. The observable effect (such meshes are not physical) is well known from track building.
8. **Order of the two new-session actions.** `lapCount` is reset by `TimeTransponder::reset`
   0x140290e60, queued onto the physics thread by `CarAvatar::onNewSession` 0x1400d91c0 (confirmed
   in review). The `Track` session lambda runs from the `PhysicsAvatar` lambda 0x1401213f0, also
   on the physics thread. Not traced: which is queued first and whether a `Track::step` can run in
   between. If the reset came first and a step ran before the session lambda, `dynamicGripLevel`
   would already have dropped back to `baseGrip` and nothing would be left to transfer.
9. **Purpose of the +10000 wall sub-spaces** and of the number prefix as a broad-phase grouping is
   my interpretation. The code only shows that they become separate `dSimpleSpace`s.
10. **`Track::rayCastWithRayCaster` callers** were not found (virtual); no call through slot +0x10
    of an `IRayTrackCollisionProvider` shows up in the pseudo-C, so it looks unused. Corrected in
    review: `NetCarStateProvider` does **not** own an `IRayCaster`; it holds an
    `IRayTrackCollisionProvider*` (`rayCastProvider`, +0x6b8) and calls slot +0x08
    (`Track::rayCast`, shared ray) every step, see 3.2. The list of `Track::rayCast` users in 3.2
    comes from a text search for the call pattern and may be incomplete.
11. **Choice made:** timing lines, starting bounds, the AI line recorder, DRS zones and the movable
    track objects are only listed here, not mapped; they are owned by `Track` but are not ground
    contact.
12. **Triangle winding for a hand-made ground quad** (section 9.2, set-up 1). Back-face culling is
    on for every ray, so a quad with the wrong winding is invisible to the wheels. Which winding
    counts as front-facing for a downward ray was not read from the binary (it sits inside
    `dCollideRTL` / OPCODE); the statement in 9.2 is unverified. An oracle should try both orders
    or copy the index order of a real track mesh.
13. **Substring matching against every key, including the built-in ones.** `ROAD`, `GRASS`, `KERB`,
    `SAND` (system file) and `WALL` are always in the map, so a track key that contains one of them
    or another key of the same file makes meshes named after the longer key ambiguous, and those
    fall back to the "no match" surface (category 0: the car's underside boxes do not collide with
    it). The rule is confirmed in `SurfacesManager::getSurface`. A key survey of the 47 track files
    in this install (review, keys only) finds two cases: `ks_silverstone/international` defines
    both `OUT` and `CONCOUT`, and `ks_nordschleife/nordschleife` defines `3dMISC`, which can never
    match because of its lower-case letter (section 4.1). Whether any mesh is really named that
    way was not checked (it needs the mesh names from the kn5 files).
14. **A car needs the track's AI line.** `SplineLocator::step`, `Car::postStep` and `Car::Car` use
    `track->aiSplineRecorder->bestLapSpline` with no null check (section 9.2). Not checked: what
    `AISplineRecorder::AISplineRecorder` loads and whether an empty or missing AI line is
    tolerated by `locateOnSpline` and `AISpline::payloadAtPosition`. Until that is known, a
    whole-car oracle should use a real track folder that has an AI line.

---

## 9. System-specific notes

### 9.1 Check of `docs/map/tyre.md` sections 4 and 6

| Statement there | Verdict |
|---|---|
| One ray per wheel per step, origin wheel centre + 2 m up, direction (0, −1, 0) | **Confirmed.** |
| Through `IRayCaster`, or `IRayTrackCollisionProvider` when there is no caster | **Confirmed.** Added detail: caster ray length 3.0 (set once in `Tyre::init`), provider path length 2.0. In a real car the caster path is always used. |
| Hit ignored unless the wheel's up axis has world-Y > 0.35 | **Confirmed** (`worldRotation.M22`). |
| Contact point = wheel centre projected onto the hit plane; beyond `acos(0.96)` the normal is rotated back and the raw hit point is used | **Confirmed**, formulas in 5.4. |
| Bumps from `sinHeight` / `sinLength`, three-sine pattern when `granularity != 0` | **Confirmed in code, but** `granularity` is always 0 here, so only the first part is live. `sinLength` is a spatial frequency, not a length. |
| Grip × `SurfaceDef::gripMod` × track `dynamicGripLevel` | **Confirmed** for both force paths and for tyre heating. |
| Section 5 table, "Loose surfaces: special case when `granularity != 0`" | Code exists, **never taken** in this build. |
| (not stated there) | **Addition:** `Tyre::step` also applies the surface `damping` drag to the car body (5.5); `Tyre::surfaceDef` is the hand-over point to the pit limiter, cut detection, collision ghosting and force feedback. |

Cross-check with `docs/map/physics_engine.md` section 3.1 (its current text already says this, so
the two documents agree): the last call in `PhysicsCore::PhysicsCore` 0x1402cba80 passes the
**world** (`this->id`, `[rdi+8]`), not the ray, to the merged 4-byte function at 0x1403404b0
(`mov [rcx+0x78], edx`, two names: `dGeomSetCategoryBits` and `dWorldSetQuickStepNumIterations`);
it is really `dWorldSetQuickStepNumIterations(world, 48)` and has no effect because QuickStep is
not used. No category or collide bits are ever set on a ray.

### 9.2 What an oracle needs to fake a flat track

Smallest set for `Tyre::step` (this is what the tyre oracle already does, and what the game's own
`TyreTester` does):

- An object with a 4-slot vtable as `Tyre::rayCollisionProvider`:
  +0x00 destructor (unused), +0x08 `rayCast(this, org*, dir*, RayCastResult*, float len)`,
  +0x10 unused, +0x18 `createRayCaster(this, float)` **returning null**, so that `Tyre::rayCaster`
  stays null and the tyre takes the provider path.
- `rayCast` fills the 0x30-byte `RayCastResult`: `surfaceDef` (+0x0) → a 0xc8-byte `SurfaceDef`,
  `pos` (+0x8) = `(org.x, groundY, org.z)`, `normal` (+0x14) = `(0, 1, 0)`, `hasHit` (+0x20) = 1,
  `collisionObject` (+0x28) = anything; return true.
  (`TyreTester::rayCast` 0x14044f270 does exactly this with `pos.x = pos.z = 0`.)
- The `SurfaceDef`: `gripMod` = 1.0, `isValidTrack` = 1, `vibrationLength` = 1.5, all other bytes 0
  (the `TyreTester` values). The pointer must not be null: `Tyre::step` reads `granularity`
  without a null check, and `stepDirtyLevel` reads `dirtAdditiveK` the same way.
- For the grip level: either `Tyre::car` = null (factor 1.0, the `TyreTester` way), or a fake
  `Car` whose `ksPhysics` points at a fake `PhysicsEngine` with `track` (+0x198) → a block with a
  float 1.0 at +0x128 (`dynamicGripLevel`).

For a whole `Car::step` on flat ground, two workable set-ups:

1. **Real `Track`, one real quad.** Build a real `PhysicsEngine` (it creates the `PhysicsCore`),
   construct a real `Track` with `Track::Track(engine, name, config)` (it reads `cfg/race.ini`
   from the documents folder and `<dataFolder>/data/drs_zones.ini`; I did not check that
   `DRSManager` tolerates a missing file, so point `name` at a real track folder), then call
   `Track::addSurface` once with 4 vertices / 6 indices of a large upward-facing quad (the author
   wrote "counter-clockwise seen from above, so it is not back-face culled"; that winding is
   **unverified**, see open question 12: try both orders), a `SurfaceDef` with
   `collisionCategory = 1`, and space id 1. `Car::Car` then gets real ray casters from
   `Tyre::init`, the underside boxes collide with the quad through `nearCallback`, and nothing in
   the car needs to know the track is fake. This exercises ODE's ray-trimesh code, so it needs ODE
   initialised.
   **Added in review:** the constructor leaves `aiSplineRecorder` null; the game fills it with
   `Track::initAISpline` 0x1402782a0 (called from `TrackAvatar::initPhysics`, after the meshes).
   The oracle must call it too before building a car, for the reason given under set-up 2.
2. **Fake `Track` object.** Put a 0x148-byte block in `engine->track` whose vtable is the 4-slot
   one above (`createRayCaster` → null) and whose `dynamicGripLevel` (+0x128) is 1.0.
   **Corrected in review:** `aiSplineRecorder` (+0x80) must **not** be null for a whole car (the
   first version of this section said null). `AISplineRecorder::getBestLapSpline` 0x1402951a0 is
   just `lea rax, [rcx+0xf0]`, and car code calls it on `track->aiSplineRecorder` without a null
   check: `SplineLocator::init` 0x1402ab2a0 stores the result as `currentSpline`,
   `SplineLocator::step` 0x1402ab5c0 (part of `Car::stepComponents`) passes it to
   `locateOnSpline`, `getSignedDistanceFromSpline` and `AISpline::payloadAtPosition` every step,
   and `Car::postStep` 0x140275430 and `Car::Car` use it as well. With a null recorder the spline
   pointer is 0xf0 and the first car step faults. So the block needs a pointer to something with
   a usable `AISpline` at +0xf0 (simplest: a real `AISplineRecorder`); whether an empty `AISpline`
   survives those calls was not checked. Null is only fine when `Track::step` is tested alone
   (section 7). Other `Track` members that car code reads must be valid too:
   at least `timeLines` (+0x28, read by `TimeTransponder::step`; an empty vector is the obvious
   choice), `drsMamanger` (+0x90, read by `DRS::step` and by
   `PhysicsAvatar::onPhysicsStepCompleted`) and `startingBounds` (+0x98, read by
   `SplineLocator::step`). How each of those behaves on empty data was not checked. With no static
   mesh the underside boxes have nothing to rest on, which is fine as long as the wheels carry
   the car. This avoids ODE collision code completely and keeps the ground analytic, which is the
   better match for a Rust port that uses an analytic plane.

In both cases `PhysicsEngine::step` must either run `Track::step` on that object (set-up 1) or the
oracle must skip it and leave `dynamicGripLevel` alone (set-up 2: with `dynamicTrack.enabled` = 0
and `isExternal` = 0 at +0xb1 / +0xb0 the real `Track::step` writes 1.0, calls the empty function,
and then calls `AISplineRecorder::step` if `aiSplineRecorder` is non-null, which it has to be for
a whole car; so on the fake block either skip `Track::step` or give it a real recorder).
