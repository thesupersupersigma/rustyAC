# Content loading: car folder, data.acd, kn5 models, CarAvatar

Source: `re/decomp/` pseudo-C (folders `CarAvatar`, `CarLodManager`, `DriverModel`, `CarHintLoader`, `KN5IO`,
`FolderEncrypter`, `SimpleFileEncrypter`, `ResourceStore`, `Texture`, `SceneGraphCloner`), `re/types/*.txt`,
`tools/re_query.py` (callees / strings / uses), the docstring and code of `tools/acd_extract.py`, and a read-only
directory listing of one installed car (`content/cars/ks_ferrari_f2004`) and of `content/driver`. Nothing was modified.

## 1. What it is

A car on disk is a folder `content/cars/<car>/` holding physics and graphics settings (`data/` as plain files, or the
same files packed into `data.acd`), several `.kn5` model files (one per level of detail plus `collider.kn5`),
`skins/<skin>/` texture overrides, an FMOD sound bank in `sfx/`, animation clips, and shadow images.
`CarAvatar` is the object that turns that folder into a car in the session. It is the graphics/logic twin of the
physics `Car`: its constructor creates the physics car, loads the models and attaches every visual and audio helper
(driver, lights, instruments, flames, skid marks, sound, on-board cameras), and afterwards it receives a copy of the
physics state every frame and moves the scene-graph nodes.
All data files are opened through `INIReader` (config.md), which silently reads from `data.acd` when the archive
exists. The archive is a flat list of named files with a per-byte additive cipher whose key is computed from the car's
folder name (`ksSecurity::keyFromString`), so it is obfuscation, not protection.
Models are read by `KN5IO::load`: a `.kn5` file is one binary stream with a header, an embedded texture table, a
material table and a recursive node tree (transform nodes, meshes, skinned meshes). Textures are created from the
embedded bytes by D3DX (so dds/png/jpg all work), cached by name in `ResourceStore`, and can be replaced by a file of
the same name in the chosen skin folder.
`CarLodManager` loads the LOD files listed in `data/lods.ini` and switches them by camera distance; `DriverModel`
loads the driver from `content/driver/` and animates steering, shifting and head movement.
Everything here runs once, on the main thread, during `Sim::Sim` (main_loop.md), except the per-frame `update` hooks.

## 2. Main classes

| Class | Source file (library) | Size | Role |
|---|---|---|---|
| `CarAvatar` | CarAvatar.obj | 0x12a8 | One car in the session (a `GameObject`). Fields include `unixName` (folder name), `configName` (optional `data_<config>`), `physics` (`Car *`), `lodManager`, `driverModel_HR` / `driverModel_LR`, `bodyTransform`, `carNode`, `guid`. Also the main API other systems use to talk to a car (about 130 small getters/setters that forward to the physics thread). |
| `CarLodManager`, `CarLodDef` | CarLodManager.obj | 0xb0 / 0x68 | LOD list from `lods.ini`; per LOD the node list and the cockpit / steering-wheel HR and LR nodes; distance switching. |
| `DriverModel` | DriverModel.obj | 0x180 | Driver mesh, skin, skeleton animation (steer, shift, paddles, head). |
| `CarHintLoader` | CarHintLoader.obj | 0x38 | Builds loading-screen hint texts for a car from its data files. |
| `KN5IO` (implements `IModelImporter`) | KN5IO.obj (KunosSim.lib) | 0xc0 | `.kn5` reader: `materialList`, `skinOverridePath` (texture folders searched first), `loadingVersion`, `root`, `bonesToSolve`; static `dlc_keys`. |
| `ResourceStore` | ResourceStore.obj (KunosSim.lib) | 0x20 | `map<name, Texture>` cache owned by the graphics manager. |
| `Texture` | Texture.obj (KunosSim.lib) | 0x28 | Handle (`kid`) + file name; thin wrapper over the kgl texture calls. |
| `SceneGraphCloner` | SceneGraphCloner.obj (KunosSim.lib) | 0x8 | Deep-copies a node tree (used for pit crew copies). |
| `FolderEncrypter` | FolderEncrypter.obj (ksCommon.lib) | static | `data.acd` reader: `decryptFile`, `fileExists`, `check`; static `keys`. |
| `SimpleFileEncrypter` | SimpleFileEncrypter.obj (ksCommon.lib) | static | Single-file variant used for special-event files. |

## 3. Entry points

### 3.1 Who creates a car

`RaceManager::initOffline` 0x14013a6c0, `RaceManager::initOnline` and `ReplayManager::load` 0x140157680 call
`Sim::addCar` 0x1401978a0 -> `CarAvatar::CarAvatar` 0x1400cd1a0 (full car with physics), then set the spawn position.
`Sim::addNetCar` 0x140197c30 -> `CarAvatar::CarAvatar` 0x1400cddc0 (remote car: same steps without `initPhysics`).
Before that, `wWinMain` has already verified the LOD files exist (`checkIfCarExists` 0x1400c02c0).

### 3.2 `CarAvatar::CarAvatar` 0x1400cd1a0, step by step (confirmed from callees and strings)

| Step | Function | What it loads |
|---|---|---|
| 1 | `CarAvatar::checkACD` 0x1400d2400 | Developer check: if both `data.acd` and `data\*.*` exist, `FolderEncrypter::check` 0x14023ba90 compares every plain file with the packed copy and prints differences. |
| 2 | `CarAvatar::initCommon` 0x1400d56e0 | `data/car.ini` via `CarAvatar::openINI` 0x1400d9630 (tries `content/cars/<car>/data_<config>/<file>` first when `configName` is set, else `data/`); `cfg/gameplay.ini`; updates the splash (`Sim::setSplashMessage`, `Sim::setSplashLoadingCar`); creates the on-screen notifier objects (brake bias, TC, ABS, turbo, engine brake, MGU-K/H). |
| 3 | `CarAvatar::init3D` 0x1400d3b90 | Scene nodes `CARNODE` / `BODYTR`; `car.ini [GRAPHICS]` (driver eyes, mirror position, `USE_ANIMATED_SUSPENSIONS`, exposure); `CarAvatar::initDriver` 0x1400d7380 (`data/driver3d.ini` -> `DriverModel`); `SuspensionAnimator` or `SuspensionAvatar`; **`CarLodManager::CarLodManager` 0x1400e2f80**; per-user seat position from `Documents/Assetto Corsa/cfg/cars/<car>/view.ini`; `CarAvatar::initMirrorMaterials` 0x1400d75f0 (`CarMirrorManager`, `data/mirrors.ini`); `VisualDamageManager` (`data/damage.ini`); `RotatingObjects` (`data/extra_animations.ini`). Prints `CAR LOADED IN %u`. |
| 4 | `CarAvatar::initPhysics` 0x1400d7660 | `new Car` (`Car::Car` 0x14026bf00: all physics ini files, see docs/map), `RaceEngineer`, `CarAvatar::initControls` 0x1400d6f30 (`cfg/controls.ini` -> keyboard / joypad / wheel control provider); loads `content/cars/<car>/collider.kn5` with `KN5IO::load` and hands the mesh to `Car::initColliderMesh` (collision shape), with a debug material; console variables. |
| 5 | `PhysicsCarStateProvider`, spawn position from `TrackAvatar::getSpawnPosition` | Connects avatar to the physics state stream. |
| 6 | `CarAvatar::initCommonPostPhysics` 0x1400d6190 | `SkidMarkBuffer` (`system/cfg/skidmarks.ini`), `TyreSmoke`, `EngineSmoke`; sound: `content/cars/<car>/sfx/<car>.bank` + `sfx/GUIDs.txt` (`AudioEngine::parseGUIDs`), `CarAudioFMOD` 0x140062830 (`data/sounds.ini`, `data/engine.ini`); `CarAnimations` 0x140060ba0 (`animations/*.ksanim`, `data/wing_animations.ini`); `GearShiftShake`; `AnalogInstruments` 0x140056480; `CarBrakeLights` 0x1400ddbb0 (`data/lights.ini`); `AnimatedLights` 0x14005a850; `ReplayRecorder`; `CarColliderRenderer`; `SuspensionGraphicsGenerator` (`data/suspension_graphics.ini`); `TyreBlur`; `BlurredObjects` 0x1400c35d0; `DigitalInstruments` 0x1400eb030; `DigitalPanels` 0x1400820f0; `Flames` 0x1400ff990; `BrakeDiscGraphics` 0x14005ca40; `CarAvatar::initCameraCar` 0x1400d47c0 (`data/cameras.ini`); `DynamicCarEffects`; user FF gain from `cfg/user_ff.ini`. |
| 7 | `Sim::unloadMeshResources` 0x14019edd0 | Frees CPU-side mesh buffers after upload. |

Later: `Sim::onPostLoad` -> `CarAvatar::onPostLoad` 0x1400d92b0 creates `CarFakeShadow` 0x1400e0d80 (`body_shadow.png`,
`tyre_N_shadow.png`, `data/ambient_shadows.ini`). `RaceManager::initGhostcar` -> `CarAvatar::initGhostCar` 0x1400d7510.

### 3.3 Per frame

- `CarAvatar::update` 0x1400db830 (GameObject vtable +0x08; render slots are empty, the car is drawn by the scene graph):
  pit-lane state, backfire, `updateFromChannels` 0x1400dd390 (lap/sector events queued by physics), body matrix
  (`makeBodyMatrix` 0x1400d8ec0), driver visibility, doors, ERS charge, skid marks.
- `PhysicsAvatar::update` (main thread) calls `CarAvatar::setNewPhysicsState` 0x1400da420 with the latest `CarPhysicsState`.
- `CarLodManager::update` 0x1400e57c0 -> `updateLodVisibility` 0x1400e5810; `DriverModel::update` 0x1400fb5d0 -> `updateHeadMovement` 0x1400fb9a0.
- Requests in the other direction (`setTyreCompound`, `setAbsEnabled`, `forcePosition`, ...) go through
  `PhysicsAvatar::executeOnPhysicsThread`.

### 3.4 Models and textures

| Function | Address | Notes |
|---|---|---|
| `KN5IO::load(filename)` | 0x1402151a0 | Prints `LOADING MODEL`. Reads 6 magic bytes, int32 `version` (error if newer than `currentVersion`), and for version >= 6 one more int32 that must be 0 or one of `KN5IO::dlc_keys`. Then `loadMaterialsBinary`, then the node tree (`loadBinaryV1` for version 1, else `loadBinaryV2`). Returns the root `Node`. |
| `KN5IO::loadMaterialsBinary` | 0x140216240 | Texture table ("Loading %d textures", each via `loadTexture`), then materials: name, shader name (`Material::setShader`), shader variables (`ShaderVariable::set`, e.g. `ksSpecularEXP`), texture slots resolved by name. |
| `KN5IO::loadTexture` | 0x1402171a0 | Texture name + embedded bytes. Cache key looks like `<kn5 file>::<texture name>` (inferred from the `::` separator literal). `getSkinOverridenTexturePath` 0x140214f90 looks for the same file name in each `skinOverridePath` folder ("SKINNED TEXTURE %S FOUND, OVERRIDING") and then uses `ResourceStore::getTexture` 0x1402000d0 (file) instead of `getTextureFromBuffer` 0x140200250 (memory). |
| `KN5IO::loadBinaryV2` | 0x140215aa0 | Recursive. Each node: class id, name (`loadString` 0x1402170f0), child count, active flag. Class 1 = transform `Node` (matrix), 2 = `Mesh`, 3 = `SkinnedMesh` (bones by name; `bonesToSolve` is resolved after the tree is complete). The three class ids and constructors are confirmed; the fields inside a mesh record (vertices, indices, material id, flags, LOD range, bounding sphere per the public format notes) were not re-derived. |
| `KN5IO::addTextureFolder` | 0x140214e90 | Adds a skin folder; callers: `CarLodManager::loadLod`, `DriverModel::DriverModel`, `PitCrew`, `LollipopCrew`. |
| `KN5IO::addDLCKey` | 0x140214de0 | Called only from `SteamInit` (five different constants; one per owned DLC pack is an interpretation). |
| `Texture::Texture` (4 overloads) | 0x1401fddc0 | -> `kglCreateTextureFromFile` 0x1400190e0 / `kglCreateTextureFromBuffer` 0x140019210 -> `KGLTexture::KGLTexture` 0x140023820, which calls `D3DX11CreateShaderResourceViewFromFileW` / `...FromMemory` (d3dx11_43.dll). There is no hand-written dds parser. |
| `CarLodManager::loadLod` | 0x1400e4370 | `content/cars/<car>/<FILE>` with texture folder `content/cars/<car>/skins/<skin>/`; then finds named nodes: wheels, suspension and brake-disc nodes (static name tables), `STEER_HR` / `STEER_LR`, `COCKPIT_HR` / `COCKPIT_LR`, `REAR_AXLE`. |
| `CarLodManager::initNoBodyNodes` | 0x1400e3fa0 | `data/proview_nodes.ini` (created if missing): nodes kept visible in "pro view". |
| `SceneGraphCloner::cloneGraph` | 0x140217a40 | Only caller: `PitCrew::PitCrew`. |

Other `KN5IO::load` callers: `GhostCar::init3D` 0x140105520, `NetCarStateProvider::NetCarStateProvider` 0x140116c70,
`TrackAvatar::initDynamicObjects`, `Sim::initStaticCubemap`, pit crew. The track model path is in track.md.

### 3.5 data.acd

| Function | Address | Notes |
|---|---|---|
| `FolderEncrypter::decryptFile(acd, name)` | 0x14023bdd0 | Walks the container and returns the named file as text, or `NOT FOUND`. Callers: `INIReader::loadEncrypt` 0x140237570, `Curve::loadEncrypted` 0x140206f50 (`.lut`), `loadGearRatiosFromFile`, `SetupGearManager::populateSpinner`. |
| `FolderEncrypter::fileExists` | 0x14023c390 | Used by `Path::fileExists` 0x1402305e0 so "does `data/x.ini` exist" also works for packed cars. |
| `ksSecurity::keyFromString` | 0x1402cfe00 | Key = eight small integers computed from the folder name, printed as `"%d-%d-%d-%d-%d-%d-%d-%d"`. |
| `SimpleFileEncrypter::decryptFile` | 0x14023b650 | `system/data/se/SPECIAL_EVENT_<guid>.acd` (callers: `RaceManager::initOffline`, `GameMode::GameMode` 0x1400bc810). |

Container layout (taken from `tools/acd_extract.py`, the repo's reimplementation of `decryptFile`; the 113 cars in
`cardata/` are its output, it was not re-run here): optional 8-byte header (int32 -1111 + int32 key id), then repeated records: int32 name length, UTF-8 name,
int32 size, then `size` int32 values whose low byte is `plain_byte + key_char[i mod key_len]`. The key string comes from
the car folder name, so a renamed folder cannot be read.

## 4. What it reads from disk

| Path | Format | Read by |
|---|---|---|
| `content/cars/<car>/data.acd` or `data/` (or `data_<config>/`) | acd container / plain ini + lut | `INIReader`, `Curve::load` 0x140206a50, `CarAvatar::openINI`, `Car::getConfigPath` 0x140270420 |
| `data/car.ini` | ini | `CarAvatar::initCommon`, `CarAvatar::init3D` (`[GRAPHICS]`, `[INFO] SCREEN_NAME`), cameras, FF form, `SetupManager` |
| `data/lods.ini` | ini: `[LOD_n] FILE, IN, OUT`; `[COCKPIT_HR] DISTANCE_SWITCH` | `CarLodManager::CarLodManager`, `checkIfCarExists`, `GhostCar::init3D` |
| `data/driver3d.ini` | ini: `[MODEL] NAME`, steer/shift/head animation settings, `HIDE_OBJECT_n` | `CarAvatar::initDriver`, `DriverModel::DriverModel` 0x1400f76d0 |
| `data/lights.ini`, `mirrors.ini`, `sounds.ini`, `cameras.ini`, `dash_cam.ini`, `analog_instruments.ini`, `digital_instruments.ini`, `digital_panels.ini`, `flames.ini`, `flame_presets.ini`, `blurred_objects.ini`, `damage.ini`, `extra_animations.ini`, `wing_animations.ini`, `suspension_graphics.ini`, `ambient_shadows.ini`, `proview_nodes.ini`, `escmode.ini` | ini | the class named after the file (addresses in 3.2); `brakes.ini` and `suspensions.ini` are also read for graphics (`BrakeDiscGraphics`, `CarLodManager::loadLod`, `SuspensionAvatar::addModel` 0x1401b32b0) |
| `<car>/*.kn5` (names from `lods.ini`), `collider.kn5` | kn5 | `CarLodManager::loadLod`, `CarAvatar::initPhysics` |
| `<car>/skins/<skin>/*.dds|png|jpg` | image files overriding kn5 textures by name | `KN5IO::getSkinOverridenTexturePath` |
| `<car>/skins/<skin>/skin.ini` | ini (driver suit / crew choice) | `DriverModel::DriverModel`, `PitCrew`, `LollipopCrew`, `TrackAvatar::initPitCrew` |
| `<car>/skins/<skin>/livery.png`, `<car>/logo.png` | png | leaderboards / ESC menu, splash |
| `<car>/sfx/<car>.bank`, `sfx/GUIDs.txt` | FMOD Studio bank + event GUID list | `CarAvatar::initCommonPostPhysics` (audio.md) |
| `<car>/animations/*.ksanim` (`steer`, `shift`, `shift_up`, `shift_dw`, `car_shift*`, `car_door_L/R`, `car_steer_extra`, `lights`) | Kunos animation clips | `DriverModel`, `CarAnimations`, `AnimatedLights` |
| `<car>/driver_base_pos.knh` | node hierarchy pose | `DriverModel` (`loadDriverBasePos` 0x1400fb0a0) |
| `<car>/body_shadow.png`, `tyre_N_shadow.png` | png | `CarFakeShadow` |
| `content/driver/<NAME>.kn5` and `<NAME>_B.kn5` | kn5 (high / low detail driver) | `DriverModel::DriverModel` |
| `content/texture/driver_*`, `Documents/.../cfg/driverskin.ini` (`SUIT`, `GLOVES`, `HELMET`) | textures, ini | `DriverModel::loadDriverSkin` 0x1400fb260 |
| `Documents/Assetto Corsa/cfg/cars/<car>/view.ini` | ini | `CarAvatar::init3D` |

`<car>/ui/` (`ui_car.json`, `badge.png`, previews) is not referenced by acs.exe (searched for `ui_car` and `/ui/`): launcher only.
For multiplayer, `ACClient::getCarMD5` 0x140046070 hashes `data.acd` or the `data/` folder (`md5Folder`).

## 5. Size

`python tools/re_query.py size` on the stems below: **350 hand-written functions, 138,119 code bytes**.

| Object | Functions | Code bytes | | Object | Functions | Code bytes |
|---|---|---|---|---|---|---|
| CarAvatar | 250 | 75,094 | | DriverModel | 23 | 20,982 |
| KN5IO | 16 | 16,106 | | CarLodManager | 36 | 14,669 |
| CarHintLoader | 2 | 4,686 | | FolderEncrypter | 5 | 3,285 |
| SimpleFileEncrypter | 1 | 1,203 | | ResourceStore | 6 | 1,196 |
| SceneGraphCloner | 6 | 552 | | Texture | 5 | 346 |

Not counted here: every helper that `CarAvatar` creates (lights, instruments, flames, audio, cameras, skid marks, damage:
renderer / audio / camera / ui areas), `Node` / `Mesh` / `SkinnedMesh` / `Material` / `KGLTexture` (renderer), `TrackAvatar`
(track.md), `Car` and everything under it (docs/map). `CarHintLoader::getCarName` / `getHints` sit in SplashScreen.obj and are
counted in main_loop.md.

## 6. Port difficulty: **L**

- Easy and already solved: the `.acd` container (`tools/acd_extract.py` is a complete reimplementation; port it 1:1, about
  100 lines). `SimpleFileEncrypter` is only needed for official special events.
- `.kn5` parsing is medium: one sequential binary read, about 16 kB of code, with public community documentation of the
  format to cross-check. No crate exists; write a small reader with `byteorder`/`nom`. Textures: use the `image` and
  `ddsfile` crates (or let `wgpu` take the dds blocks) in place of D3DX.
- The large part is `CarAvatar` (75 kB, 250 functions): it is glue to about 40 helper classes and to the physics thread.
  The loading sequence in 3.2 is straightforward, but it can only be completed as the helpers get ported; the many
  one-line getters/setters disappear if the Rust design shares state differently.
- `DriverModel` (21 kB) needs skeletal animation (`.ksanim`, `.knh`, bone solving): unknown formats that the renderer area
  must map before this can be ported; a first version can skip the driver.
- For a physics-only milestone none of this is needed except the acd/ini path and `collider.kn5` (mesh for the car's
  collision shape), which requires only the mesh part of the kn5 reader.
- Third-party involved: D3DX11 (texture decode), FMOD Studio (bank loading), Steam (DLC keys for kn5).
- Depends on: config (INIReader, Path), renderer (Node / Mesh / Material / shaders / ResourceStore owner), audio, physics
  (`Car`), main loop (`Sim`, `PhysicsAvatar`), track (spawn positions).

## 7. Open questions

- kn5 details were skimmed, not decoded: exact field order inside mesh and skinned-mesh records, the meaning of the 6 magic
  bytes, the highest supported version (`currentVersion`), and what the DLC key actually gates (only an equality test against
  `dlc_keys` was seen; no decryption call was noticed in `KN5IO::load`).
- `KN5IO::loadBinaryV2` lists `loadBinaryV1` among its callees; whether that is real recursion into the old format or an
  artefact of the call index was not checked.
- The `.acd` header's "key id" (second int32 after -1111) and `FolderEncrypter::keys`: `acd_extract.py` skips the id and
  still produced readable data for the cars in `cardata/`, so the table's purpose is unknown.
- Whether textures inside kn5 are always stored as complete image files (dds/png) is inferred from the use of
  `D3DX11CreateShaderResourceViewFromMemory`; not verified on a file.
- How the skin name reaches `CarLodManager::loadLod` (from `race.ini [CAR_n] SKIN` through `CarAvatar`) was not traced line by line.
- `.ksanim` and `.knh` formats are unmapped (belongs with the renderer / animation area).
- Differences between the two `CarAvatar` constructors beyond "no `initPhysics`" (state provider used for remote cars,
  `NetCarStateProvider`) were not read.
- Choice made: the helper classes created by `CarAvatar` are listed here by name and file only and are sized in their own areas.
