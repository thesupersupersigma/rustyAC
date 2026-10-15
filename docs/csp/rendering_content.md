# Rendering: content-side hooks (cars, tracks, lights, effects)

CSP does not own a car or track loader; it wraps the loaders and the per-object update / draw calls of acs.exe's own
content classes (`CarAvatar`, `TrackAvatar` and the 35 small objects the car and the track hang on themselves) and
adds a second layer of content features (new lights, flames, real mirrors, tyre FX, dynamic objects, grooves and
spawn-point repair, weather-driven sun). This file maps the **140 hook-table rows of those classes** (118 hooks +
22 lookups, 115 distinct replacement functions in `dwrite.dll`, image base 0x180000000) and the content-side code that
sits behind them. The configuration language itself (INIpp, light / material / shader sections) is in
`rendering_config.md`, the shared render seams (GraphicsManager, kgl, KN5IO, Material) in `rendering_core.md`,
the Lua host in `lua.md`; nothing of those is repeated here.

Class legend (same as `rendering_core.md`): **A** pass-through, **B** wraps (pre / post work, a signal, a temporary
override, then the original), **B\*** wraps but the original is skipped under a gate (a flag, a cancelled signal, a
file that is missing), **C** replaces (the original is never called), **D** lookup (CSP only needs the address).
`[interp]` = inference; `[dll]` = read in a decompile or string table (RVA given); `[log-csp]` = the owner's
`custom_shaders_patch.log` of 2026-10-12 (monaco_2017 + rss_formula_2010, offline race).

## 1. What it does

1. **Everything is a thin wrapper that opens a seam.** Of 118 hooks, **65 are B, 33 are B\*, 20 are C and none is A**
   (the 22 lookups are D). The replacement bodies are tiny: the 115 replacement functions together have only 43,846
   bytes of `.pdata` function body (316,985 bytes of raw Ghidra pseudo-C), and the biggest are BrakeDiscGraphics::update
   (4,431 B), TrackAvatar::init3D (4,049), TyreSmoke::update (3,769), CarAvatar::init3D (2,961). The real content logic is
   in ordinary CSP functions behind them (section 2.7). `[dll]`
2. **Signals instead of logic.** 20 of the replacements walk a vector of listeners (0x28-byte entries, callback at +8,
   walked last to first, "cancel" byte in the argument block) before calling the original; 18 distinct cells (RVAs in
   2.2). The listeners are CSP's own modules (Lua bridge, Extra FX, smart shadows, WeatherFX ...), registered elsewhere,
   so what a cell *does* is not visible from the hook. `[dll]`
3. **A per-car "load context" and a per-car extension object.** While a car is being built a global pointer (RVA
   0x26a9d50, "load context") is set; the constructors of AnimatedLights, BrakeDiscGraphics, CarBrakeLights,
   Flames, TyreBlur, VisualDamageManager, DynamicCarEffects, CarMirrorManager and each TyreSmoke store `this` into
   fixed slots of it (2.1). Later every hook finds the car's CSP-side object by `carIndex` (CarAvatar+0x1158) in a
   vector of 8-byte pointers (RVA 0x26ae278 begin / 0x26ae280 end) and reads car state, per-wheel arrays and the car's
   config values from it. `[dll]`
4. **The car config is parsed from inside `CarAvatar::init3D`.** The hook (0x10ac5d0) makes sure the car's config chain
   is loaded (loader 0x14900d0, idempotent), then asks the config for `[BASIC] LOAD_SEATBELT_OFF` and, if set, flips
   the global "load inactive seatbelt" switch for the duration of the original `init3D`; after the original it finds
   the seatbelt nodes `CINTURE_ON` / `CINTURE_OFF` by name and registers them with the LOD manager. In the log the
   same loader has already run when tyre types are detected ("Loading configs for rss_formula_2010, skin: 01_rss_red_7
   ... loaded, time: 225.56 ms, in total 1396 values", then "[ConfigsCache] Storing cached version"), i.e. it is
   lazy: whichever hook asks first parses the chain. `[dll][log-csp]`
5. **Car load keeps AC's order but adds guards and extra work per LOD.** `CarLodManager::loadLod` is wrapped to log
   "Load LOD: <file>" and to *skip* a LOD whose kn5 does not exist ("Car model is missing: ..." at log level
   error) instead of crashing; after each LOD the log shows CSP's per-LOD passes (windscreen detection, mirror UV
   normalisation, wiper element ids, rain occlusion baking about 0.5-1.1 s per LOD, mirror splitting, shadow-caster
   generation from the last LOD) which live in the 0x14xxxxx region, not in the hook. `[dll][log-csp]`
6. **Per-frame effects of cars are throttled by a rank.** `CarNodeSorter::sort` is replaced by a stable insertion
   sort of the car nodes by distance with a 2 % hysteresis, and then writes a rank (CarAvatar+0xfc4; -1 for cars
   that are not active) that other hooks use: tyre smoke only for ranks <= `LIMIT_SMOKE2` (default 3),
   `PostProcessEffectsUpdater` only for ranks < 4 (10 with a quality flag) within 200 m (800 m), `DriverModel` head
   sway only for ranks < 8, and a "distance class" byte (CarAvatar+0xfe2 / +0xfe6, set elsewhere) that makes
   CarAnimations, BrakeDiscGraphics, TyreBlur, DriverModel, DynamicCarEffects, VisualDamageManager skip their update
   for far cars when `LIMIT_GENERAL2` is on. `[dll]` flag names `[interp]`
7. **Track repair at load.** After the original `TrackAvatar::init3D` CSP applies the *grooves fix* (decal meshes
   such as skid marks, grooves, water are moved behind a reference mesh; log: "22 meshes, 3 materials ... moving
   behind commis_b01"), `TrackAvatar::initRespawnPositionSet` creates missing spawn points (`Creating a missing spawn
   point AC_HOTLAP_START_1 ... _21` on monaco_2017), `processPhysicsNode` refuses digit-only / vertical meshes,
   `initPhysics` computes the pit altitude ("Pit altitude for current track: 8", "Pit Y coordinate: -13.3419"), and
   `initDynamicObjects` seeds the random numbers of dynamic objects and recognises balloons. `[dll][log-csp]`
8. **Sky.** `SunAnimator` and `WeatherManager` are only gates: when WeatherFX (the Lua weather) is active the vanilla
   sun update and the "apply custom weather" file loading are skipped, so the +/-80 degree clamp of AC's sun is gone
   (details of the sky itself: `rendering_config.md` 3.3 / 3.4). `[dll]`
9. **Four hooks are deliberately empty.** `CarAvatar::checkACD`, `CarFakeShadow::generateFakeShadow`,
   `CarLodManager::updateLodVisibility`, `VirtualMirrorRenderer::update` are all registered with the shared empty
   function 0x2db1f0 (a lone `ret`, named `_guard_check_icall` by Ghidra, shared with 36 other no-op rows of the
   table: destructors, `SharedMemoryWriter::*`, `SteamInterface::*`, `Sim::unloadMeshResources`, ...). Those four
   acs.exe behaviours are therefore **switched off**, not wrapped. `[dll]`

## 2. Game parts it replaces or extends

### 2.1 Shared machinery seen in these hooks

| Item | RVA (VA = base + RVA) | Meaning | Evidence |
|---|---|---|---|
| Load context pointer | 0x26a9d50 | Non-null while a car is built. Slots: +0x20 AnimatedLights, +0x28 BrakeDiscGraphics, +0x30 CarBrakeLights, +0x48 Flames, +0x60 TyreBlur, +0x68 VisualDamageManager, +0x70 DynamicCarEffects, +0x78 CarMirrorManager, +0x80..+0x90 vector of TyreSmoke*, +0xa0 object whose +0x440 is the car's extension data, +0xe8 / +0x100 car path string. Also read by 80+ physics hooks (`attach.md` 1.7 says it doubles as the single-car debug override). | `[dll]` ctor hooks of 8 classes |
| Per-car extension vector | 0x26ae278 / 0x26ae280 | begin / end of 8-byte pointers indexed by car index. Fields seen: +0x2e9 "active in scene", +0x2ea, +0x2f2 "mirrors allowed", +0x364 / +0x36c smoothed lateral / longitudinal acceleration, +0x440 -> extension data (+0x108 / +0x10c animation speed and door limit, +0x130 "new flames disabled", +0x390 neck G factor, +0x484 steer value, +0x4b1), +0x548 -> per-wheel table (0xe8 stride from +0x5c8), +0x27c + string at +0xc0 "custom gear name" (R / N / P / digit), +0x50 CarFakeShadow, +0x204 / +0x1f4 / +0x214 positions | `[dll]` offsets, meaning `[interp]` |
| acs.exe CarAvatar fields CSP touches | n/a | +0x1158 index, +0x1168 physics `Car*`, +0x1008 remote-car data (non-null = remote), +0x1010 LOD manager, +0xfc4 rank, +0xfc5, +0xfc7 "controls locked by CSP", +0xfe0 / +0xfe1 HR/LR driver switch, +0xfe2 / +0xfe6 distance class, +0xfe3 flag byte (bit0 freeze driver-model-and-friends during update, bit1 skip new physics state, bit2 "extrapolated this frame", bit3 skip damage level), +0xfe4 extrapolation override (0 auto, 2 force), +0xfe8 / +0xff0 DriverModel HR / LR, +0x1250 / +0x1258 seatbelt OFF / ON nodes, +0x1260.. four compound indices, +0xe18 audio multiplier object | `[dll]` |
| Launch-state getter | 0xef1700 (98 B) | Returns a pointer to a small struct of launch flags; byte 0 (CM showroom / preview mode `[interp]`), byte 3 ("replay / hotlap / headless": forces SkidMarkBuffer size 8, disables FFB), byte 5 (preview state file). Used by CarLodManager::loadLod, CarAnimations::update, SkidMarkBuffer ctor, TrackAvatar::initPitCrew, init3D | `[dll]` + `physics_car_hooks.md` |
| "Render pass active" / mirror flags | 0x268b685, 0x268b686, 0x268b687 | set by `GraphicsManager::beginMainRenderPass` (0x117eb20) and by `MirrorTextureRenderer::render`; read by CarFakeShadow::onNodeRenderEvent | `[dll]` |
| Wind vector | 0x2698158 (valid flag 0x26a977c) | vec2 wind written by a listener of cell 0x26974e8; consumed by TrackAvatar::update / updateDynamicObjects for balloons | `[dll]` |

### 2.2 Signal cells fired by these hooks (RVA of the vector pointer; `x` = cancels the original)

| Cell | Fired by | Position | Cancel |
|---|---|---|---|
| 0x2696868 | CarAvatar::init3D | before | no (argument: avatar, config path string) |
| 0x2696850 | CarAvatar::init3D | after | no |
| 0x2696858 | CarAvatar::updateSkidMarks | before | x |
| 0x2696870 | CarAvatar::setDriverInfo | before | x |
| 0x2696878 | CarAvatar::lockControlsUntilTime | before | no |
| 0x2695f48 | CarBrakeLights::update | before | x (Lua redirection of brake lights) |
| 0x268ff40 | BrakeBiasNotifier::update | before | x |
| 0x2690b20 | CarFakeShadow ctor | after | no |
| 0x2690b28 / 0x2690b30 | CarFakeShadow::onNodeRenderEvent | before / after | x / no |
| 0x2690b48 | DamageDisplayer::renderHUD | before | x (`ac.redirectDamageDisplayer`) |
| 0x2690b80 | DriverModel::update | after | no |
| 0x2690b88 / 0x2690b90 | DriverModel::updateHeadMovement | before / after | x / no |
| 0x2690bf0 | FuelLight::renderHUD | before | x |
| 0x26910c0 | SkidMarkBuffer ctor | before | no (argument: pointer to the buffer size) |
| 0x2697770 / 0x2697778 | MirrorTextureRenderer::render | per mirror pass / after the loop | no |
| 0x26974c8 | SunAnimator::update | before | x |
| 0x26974e8 | TrackAvatar::update / updateDynamicObjects | before (wind query) | no |
| 0x2697520 | TrackAvatar::initPitCrew | before | x |
| 0x2697ad0 | VirtualMirrorRenderer::renderVirtualMirror | before | x (`ac.redirectVirtualMirror`, `ac.postponeVirtualMirror`, `ac.disableVirtualMirror`) |
| 0x2691150 / 0x2691158 / 0x2691160 | Flames::loadFlames | a callback cell temporarily swapped around the original (a `std::function` triple) | n/a |

`[dll]` all; the Lua names next to the cancel marks are `[interp]` matches by name with `lua_sdk_functions_union.txt`.

### 2.3 CarAvatar (41 rows: 28 hooks, 13 lookups; `CarAvatar.obj`; `docs/atlas/content_loading.md` 3.2 / 3.3)

| acs.exe symbol | acs VA | CSP RVA | Class | What changes |
|---|---|---|---|---|
| CarAvatar::CarAvatar (overload 1) | 0x1400cd1a0 | 0x10ab7a0 | B | Calls 0x10ab7e0 (1,081 B: wraps the original and performs the online input-method check: "Chosen input method is not available on this server, only %ls allowed", "none of input methods are allowed", gamepad / keyboard) and then 0x10af030 (post-construct: creates the car's extension object). Which overload this is is not resolved (both rows have the same VA in the table) |
| CarAvatar::CarAvatar (overload 2) | 0x1400cd1a0 | 0x10abc20 | B | Sets a per-thread short (TLS +0x1b4) from a global (0x2695176 / 0x2695178) for the duration of the original, then runs 0x10ab2b0 and 0x10af030. Helper 0x10ab3f0 (909 B) runs before |
| CarAvatar::checkACD | 0x1400d2400 | 0x2db1f0 | C | **no-op**: the developer comparison of `data/` with `data.acd` is gone |
| CarAvatar::forcePosition | 0x1400d2f30 | 0x10ad390 | B | For car 0 with a non-zero jitter vector (0x26a9698 +4): adds +-0.5 * random * amount to the target translation and a random yaw wobble before the original (position randomisation of a placed car `[interp]`) |
| CarAvatar::getCurrentCompoundShortName | 0x1400d3200 | 0x6f4c40 | B | After the original: looks up the car's extension map (hash key 0xafeab161bca85cae) and overrides the returned name |
| CarAvatar::getTyreCompound | 0x1400d3980 | 0x10aac80 | C | Returns the compound name from the extended compound table (0x3f0-byte entries at physics+0x1028) instead of AC's fixed list |
| CarAvatar::goToSpawnPosition | 0x1400d3ae0 | 0x10aaf00 | B\* | Teleport-to-pit rules: with `[EXTRA_RULES] ENFORCE_BACK_TO_PITS_STOP` (server) refuses while speed > 1 km/h ("Stop your car to teleport to pits"); `ENFORCE_BACK_TO_PITS_PENALTY` shows "Teleporting to pits is not allowed in this race" / "Teleporting is not allowed"; otherwise calls the original |
| CarAvatar::init3D | 0x1400d3b90 | 0x10ac5d0 | B | See section 1 item 4: cell 0x2696868, memory scope "Car #%d/Model", config chain, `LOAD_SEATBELT_OFF`, original, `CINTURE_ON/OFF` lookup (+0x1258 / +0x1250), visibility from launch flag byte 0, cell 0x2696850 |
| CarAvatar::initCommon | 0x1400d56e0 | 0x10abdd0 | B | Sets global byte 0x26a756c = 1 around the original (a "inside initCommon" flag read by the INIReader hooks `[interp]`) |
| CarAvatar::initCommonPostPhysics | 0x1400d6190 | 0x10aeba0 | B | Memory scope "Car #%d/Extras" (only when `ESTIMATE_MEMORY_CONSUMPTION`) |
| CarAvatar::initPhysics | 0x1400d7660 | 0x10ae9b0 | B | TLS byte +0x1ba = 1 during the original; scope "Car #%d/Physics" |
| CarAvatar::isAbsInAction | 0x1400d8ae0 | 0x10aed20 | C | True when the car has physics and `+0x4cc > 0.1` and CSP's function 0x563fb0 says the ABS (physics+0x958) is working (CSP's own 4-wheel ABS) |
| CarAvatar::isInPit | 0x1400d8bd0 | 0x10aab30 | B\* | With `EQUALIZE_PITS_CHECK`: pit state = squared distance to the pit box position < 2.25 (1.5 m) instead of AC's own test |
| CarAvatar::lockControlsUntilTime | 0x1400d8e20 | 0x10aabe0 | B | cell 0x2696878 then original |
| CarAvatar::onNewSession | 0x1400d91c0 | 0x10ae8d0 | B | clears global 0x268aa54 during the original (and restores) |
| CarAvatar::resetMultVolume | 0x1400d9ab0 | 0x10ae910 | C | Lazily creates a 0x4b8-byte audio-multiplier object (car+0xe18) and writes 1.0 to its two multipliers (see `audio.md`) |
| CarAvatar::setAbsEnabled | 0x1400d9bc0 | 0x10abee0 | B | Remembers the user's ABS toggle of car 0 (globals 0x25e0720 / 0x25e0728); reads race.ini `[ASSIST]` ABS value (0 off / 1 factory / 2 user) and, with `PREFER_DISABLED_ABS2`, keeps ABS disabled even if the server forces it; then original |
| CarAvatar::setAutoBlip | 0x1400d9c50 | 0x10abe30 | B | Stores the user's auto-blip state of car 0, queues a physics-thread task (flag 0x800000), then original |
| CarAvatar::setAutoClutchEnabled | 0x1400d9ce0 | 0x10ac580 | B | Stores the user's auto-clutch state (0x25e0730 / 0x25e0734) |
| CarAvatar::setControlsLock | 0x1400d9f00 | 0x10ad370 | B | Forces "locked" while car+0xfc7 (lock by CSP) is set |
| CarAvatar::setDamageLevel | 0x1400da020 | 0x10abe10 | B\* | Skipped when flag bit3 (+0xfe3) is set |
| CarAvatar::setDriverInfo | 0x1400da0b0 | 0x10ad5e0 | B\* | cell 0x2696870 (cancel) then original; also registered as a lookup |
| CarAvatar::setNewPhysicsState | 0x1400da420 | 0x10ae290 | B | `EXTRAPOLATE_STATE` (general.ini [OTHER_EXPERIMENTS], shipped 4): after the original (or instead of it for flag values) shifts position of the avatar and of the four wheels by `velocity * clamp(simTime - stateTime, +-3 s (+-23 s with a flag))`; also floors six angular-velocity floats at a tiny non-zero value (0x0da24260); skipped when flag bit1 |
| CarAvatar::setTcEnabled | 0x1400da950 | 0x10ac2b0 | B | Same as setAbsEnabled for traction control (`PREFER_DISABLED_TC2`, race.ini `TRACTION_CONTROL`) |
| CarAvatar::setTurboBoost | 0x1400da9e0 | 0x10ae1f0 | B\* | Ignored while global 0x268bcb0 is set (set by `Sim::onKeyDown` when `[CONTROL] UNBIND_TURBO_DIGITS` and a digit key was pressed) |
| CarAvatar::setVisible | 0x1400dade0 | 0x10abdf0 | B | A "hide" request is turned into "show" while global 0x268b415 is set |
| CarAvatar::update | 0x1400db830 | 0x10ad200 | B | With flag bit0: nulls driver models (+0xfe8 / +0xff0), +0x1c0, +0x11c0, +0x1068 for the original call (skips driver / audio-side parts for that car) |
| CarAvatar::updateSkidMarks | 0x1400dd780 | 0x10ad2d0 | B\* | cell 0x2696858 (cancel: CSP's new skid marks take over) |
| BackfireParams::checkBackfire | 0x1400d26b0 | 0x58c120 | B\* | Original only if the car's audio-multiplier object exists and its byte +0x4b1 is 0 |
| BrakeBiasNotifier::update | 0x1400db510 | 0x58e050 | B\* | cell 0x268ff40 (cancel) |
| (lookups, class D) | | | D | cycleERSHeatCharging 0x1400d29e0, cycleERSPower 0x1400d2a80, cycleERSRecovery 0x1400d2bc0, getRoadDirection 0x1400d3630, makeBodyMatrix 0x1400d8ec0, modifyUserFFGain 0x1400d90f0, setAutoShifter 0x1400d9d50, setBlackFlag 0x1400d9e70, setControlsProvider 0x1400d9f90, setDriverInfo (2nd site), setGentleStop 0x1400da280, setHeadlights 0x1400da340, setTyreCompound 0x1400dabd0; also MGUHModeNotifier::update 0x1400dc170. CSP calls them from its Lua bridge (`ac.setHeadlights`, `physics.setGentleStop`-like names, ERS cycling) |

### 2.4 Car sub-objects (hooks of the objects `CarAvatar::initCommonPostPhysics` creates)

| acs.exe symbol | acs VA | CSP RVA | Class | What changes |
|---|---|---|---|---|
| CarLodManager::CarLodManager | 0x1400e2f80 | 0x10923c0 | B | After the original: if the LOD vector (+0x60) is empty, builds a default LOD list from `car.ini` / `lods.ini` / `@dummy` (0x1092290, 292 B); sets index 0xffff; asks the load context for the car data |
| CarLodManager::loadLod | 0x1400e4370 | 0x1092fa0 | B\* | Missing kn5: log "Car model is missing: <file>" and return; else log "Load LOD: <file>", set global 0x25e7c64 = LOD out distance for the duration of the original. With launch flag byte 0 and out distance 0, the out distance becomes ~1e10 |
| CarLodManager::update | 0x1400e57c0 | 0x10944a0 | B\* | Car 0 with a Sim flag (+0x1e0) -> original; else forces every node of the "hidden by LOD" vector visible and empties it |
| CarLodManager::updateLodVisibility | 0x1400e5810 | 0x2db1f0 | C | **no-op** (LOD switching is done by CSP) `[interp]` |
| CarLodManager::initNoBodyNodes | 0x1400e3fa0 | lookup | D | `proview_nodes.ini` helper called by CSP |
| CarMirrorManager::CarMirrorManager | 0x1400e5f20 | 0x10b04d0 | B | Registers in load context +0x78; sets a global during the original; for each mirror material finds the shader variable `ksEmissive` (1,1,1) and the extended flag +0xad |
| CarMirrorManager::update | 0x1400e6fc0 | 0x10b0720 | C | Full replacement: after frame 9, per mirror chooses between two materials (real mirror texture vs. AC's default) according to car+0x2f2, the camera mode (_DAT_1826aa830+0x120 is 0 or 2 with a particular sub-state) and `ACTIVE_ON_FOCUSED`; looks up the `CarMirrors.CustomMirror` flag of the car and sets the mesh pass bits (+0x14c = 0x3000000 or 0); bumps a refresh counter |
| CarNodeSorter::CarNodeSorter | 0x14006bb60 | 0x10b0db0 | B | Sets node class byte (+0xb5) to 3 |
| CarNodeSorter::sort | 0x14006be30 | 0x10b12b0 | C | Insertion sort with 2 % (min 1.0) hysteresis of the car nodes by distance, or a full sort (0x10b1e80) when flag 0x26a9186 is set; then writes car ranks (+0xfc4; -1 if +0x2e9 is 0). The rank feeds LIMIT_SMOKE / effects |
| CarAnimation::CarAnimation | 0x140060b40 | 0x5a3ee0 | B | post-construct 0x5a3f10 |
| CarAnimations::CarAnimations | 0x140060ba0 | 0x10e4230 | B | zero flags (+0xa4), post-construct 0x10e4440 |
| CarAnimations::setDoorOpen | 0x140062140 | 0x10e4260 | B\* | skipped when flag bit0; during the first 10 frames the "open" request is queued to the main-thread task queue (0x11bfc40) instead of acting immediately |
| CarAnimations::update | 0x140062160 | 0x10e42c0 | B\* | Skipped when dt is 0 or (LIMIT_GENERAL and far car). Early frames: with launch flag byte 0 resets door time (+0xa0); later: clamps the door animation time to `ext+0x10c`. In camera mode 9 or flag bit1 the three animation lists (+0x60..+0x70) are hidden for the original call |
| AnimatedLights::AnimatedLights | 0x14005a850 | 0x10e1ee0 | B\* | Registers in load context +0x20. If CSP's config uses the same `lights.ksanim` (check 0x10e1a20) logs "Animation lights.ksanim is used by CSP animation; disabling AC headlights animation", clears the vector and **frees the object (returns null)** |
| AnimatedLights::update | 0x14005aef0 | 0x10e1fc0 | B | divides dt by `ext+0x108` (animation speed), then original |
| CarBrakeLights::CarBrakeLights | 0x1400ddbb0 | 0x10910b0 | B | Registers in load context +0x30 |
| CarBrakeLights::update | 0x1400e0450 | 0x10910e0 | B\* | cell 0x2695f48 (cancel) |
| CarBrakeLights::loadLights | 0x1400de0b0 | lookup | D | |
| Flames::Flames | 0x1400ff990 | 0x10989b0 | B | clears "disabled" byte (+0x154) |
| Flames::checkTextures | 0x140100c40 | 0x1098a20 | C | Only ensures the texture vector has one zeroed 0x68-byte entry |
| Flames::cycleTexture | 0x140100f00 | 0x10989e0 | B\* | Only while not paused, camera mode != 0xb, flame intensity (0x26adb40+0x138) != 0 and `0x268b6a4` clear |
| Flames::drawFlame | 0x1401010a0 | 0x1098e40 | B | Forces texture index 0x167 (an out-of-range "no flame") when the same conditions fail |
| Flames::loadFlames | 0x140101f90 | 0x1098ad0 | B | Registers in load context +0x48, swaps the callback cell 0x2691150..60 (a loader closure 0x1098d50) around the original; empties the second vector (+0xf0) when the first is empty |
| Flames::loadTextures | 0x140103620 | 0x1098f40 | B\* | When `particles_fx.ini [FLAMES] ENABLED` (flag 0x269514d, set only when `PREVENT_CREATING_UNNECESSARY2`) and the car's `DISABLE_NEW_FLAMES_ENTIRELY` is off: AC's texture loading is skipped and one placeholder is created |
| Flames::onNodeRenderEvent | 0x140103aa0 | 0x1098e90 | B | Adds the scene origin shift (0x25e2f08..10) to the car's flame node translation during the original |
| Flames::update | 0x1401046a0 | 0x1098dc0 | B\* | If disabled byte: zeroes every flame state (stride 0x38); with `[DEV] FLAMES_ALWAYS_ON` writes constant parameters; else original |
| BrakeDiscGraphics::BrakeDiscGraphics | 0x14005ca40 | 0x58e0f0 | B | Registers in load context +0x28 |
| BrakeDiscGraphics::update | 0x14005d930 | 0x58e130 | C | Full re-implementation: at first call reads `brakes.ini [DISCS_GRAPHICS]` lag constants (LAG_HOT default 0.995), finds the disc meshes **by shader** (`shader:ksBrakeDisc?`) under each `WHEEL_*` node instead of AC's fixed `DISC_*` names, supports several meshes per wheel, and each frame writes `glowLevel` (temperature driven, lagged) and `blurLevel` (wheel speed * 0.1) straight into the material constant memory with a dirty flag. Skipped for far cars with LIMIT_GENERAL |
| MaterialVar::setFloat | 0x14005d8d0 | 0x623660 | B | After the original, if the "material var counting" global (0x268adcb) is set and the var is not the dummy var ("csp:dummy & dead", 0x623450), decrements its use counter (+0x80); BrakeDisc / VisualDamage code increments the same counter to protect vars it drives |
| TyreBlur::TyreBlur | 0x1401cf760 | 0x6b8c70 | B | Registers in load context +0x60, original |
| TyreBlur::update | 0x1401cfdb0 | 0x6b8d30 | B\* | skipped for far cars with LIMIT_GENERAL |
| BlurredObjects::BlurredObjects | 0x1400c35d0 | 0x58d620 | B | post-construct 0x58da60 |
| BlurredObjects::update | 0x1400c43b0 | 0x58d650 | C | For each blurred mesh, visible when `min <= |blur factor * wheel speed| <= max`; blur factor 0 if `0x268a7ae` set, else the camera's multiplier; calls a refresh (0x119d460) when a mesh turns visible and `0x268b43e` is set |
| TyreSmoke::TyreSmoke | 0x1401d0000 | 0x11cc1e0 | B | Appends `this` to the load-context smoke vector (+0x80), original, then post-construct 0x11cd140 |
| TyreSmoke::update | 0x1401d0b80 | 0x11cc280 | B | Gated (not paused, camera mode != 0xb, `rank <= LIMIT_SMOKE2` or fewer than 13 active smokes). Computes a strength from the wheel's slip / speed / surface in the per-wheel table (+0x5c8, +0x5cc, +0x578, surface +0x5bc) and, when > 0.5 *and* the smoke strength test passes, temporarily overrides about ten emitter parameters (colour 0x3e800000 / 0x3eb33333 / 0x3ee66666, sizes 0.5 / 0.2 / 2.0, life 3.0, scales 0x41100000, 1.0, 60.0 ...) with scope guards, calls the original with the world position shifted by the origin shift, and bumps the active-smoke counter |
| SkidMarkBuffer::SkidMarkBuffer | 0x14018f2d0 | 0x6a0710 | B | cell 0x26910c0 may change the buffer size; with launch byte 3 the size is forced to 8; sets node class 0x11 and flags; post-construct 0x6a0e30 |
| SkidMarkBuffer::render | 0x1401905e0 | 0x6a07f0 | C | Full replacement: skipped if a flag of the render target (+0x1c4) is set or fewer than 4 vertices; otherwise sets render targets / states through the D3D11 context vtable (offsets 0x90, 0x120) and draws its own geometry (0x180ea8700). `dclSkidmarks*` shaders (`rendering_config.md` 3.7) |
| CarFakeShadow::CarFakeShadow | 0x1400e0d80 | 0x5a3f30 | B | Registers in the extension object (+0x50), cell 0x2690b20 |
| CarFakeShadow::generateFakeShadow | 0x1400e1430 | 0x2db1f0 | C | **no-op** (CSP's `fake_shadows` replaces the baked texture) |
| CarFakeShadow::onNodeRenderEvent | 0x1400e22b0 | 0x5a4c60 | B\* | Own visibility tests (car blown flag `-1e20` at +0x9e8, frustum test 0x108e430 / 0x1ebffc0, `LIMIT_SHADOWS2` + rank < 3), cells 0x2690b28 (cancel) / 0x2690b30 before / after the original |
| DynamicCarEffects::DynamicCarEffects | 0x140091230 | 0x5bff90 | B | Registers in load context +0x70; initial value -1.0 (+0x84) |
| DynamicCarEffects::update | 0x140091ac0 | 0x5c0370 | C | A smoothed value in [-1, 1] integrated from the four wheels (wheel dirt * 0.3, surface class 7 -> -0.1, class 8 -> -1.0, speed above 4 units, car speed - 1 clamped 0..40) times four per-wheel gains, damped by a nearby-object term; written to a float in each registered mesh's material var memory when it changed by > 0.005. An external override (global 0x25a2cd4 >= 0) replaces it |
| VisualDamageManager::VisualDamageManager | 0x1401d2890 | 0x6ba530 | B | Registers in load context +0x68; post-construct 0x6bae70 |
| VisualDamageManager::initScratchableParts | 0x1401d5320 | 0x6ba620 | B | After the original: reads `damage.ini [SCRATCHES] MIN_SPEED / MAX_SPEED`, finds meshes whose material has the new property `damageZones` ("materialProperty:damageZones") and raises their var use counter, so CSP's shaders can read damage zones |
| VisualDamageManager::update | 0x1401d56f0 | 0x6ba570 | B | If global 0x25a2cf0 >= 0 the five damage values (+0x9c4..+0x9d4) are forced to it for the original call; else original unless far car (LIMIT_GENERAL) or `0x26b8a18` (headless) |
| VisualDamageManager::~VisualDamageManager | 0x1401d4100 | lookup | D | |
| DriverModel::DriverModel | 0x1400f76d0 | 0x5b7fd0 | B | post-construct 0x5b9e60 (driver model replace / HIDE_OBJECT handling `0x14eabf0`) |
| DriverModel::update | 0x1400fb5d0 | 0x5b9210 | B | Temporarily overrides car fields for the original: camera mode forced to 5 (flag bit2), steer value (+0x4c4) from the extension's steer (+0x484), gear index (+0x4d4) from the extension's custom gear name (R -> 0, N -> 1, P -> 2, else digit), seat position (+0x29c/+0x2a4) for flag `+0xfe1`; far cars: forces animation time 0.5; then cell 0x2690b80 |
| DriverModel::updateHeadMovement | 0x1400fb9a0 | 0x5b98d0 | B\* | If the model belongs to the focused car (iRam25e9810) and the camera is the cockpit camera (vtable check of CameraOnBoard / CameraOnBoardFree): head matrix = camera orientation (quaternion), and the original is skipped; else cells 0x2690b88 (cancel) / 0x2690b90, original, then for rank < 8 an extra G-force head sway (0x390 gain, clamp 0.01) |
| DriverModel::setVisible | 0x1400fb5a0 | 0x5b9e30 | C | Writes the node active byte (+0xd8) directly; `+0xb8` "never show" byte wins |
| DisplayNode::DisplayNode (x2) | 0x1400f6d40 | 0x5b5680, 0x5b56b0 | B | Sets node class byte 10, post-construct 0x5b5820 |
| DisplayNode::render | 0x1400f74a0 | 0x5b56e0 | B\* | Original only when the render event is not a mirror/secondary pass (event flag byte +0xe bit0 clear and type == 1) |
| FuelLight::FuelLight / renderHUD | 0x1400bb510 / 0x1400bc030 | 0x5dbdb0 / 0x5dbdd0 | B / B\* | stores `this`; renderHUD: cell 0x2690bf0 (cancel), object update when `0x1cf110` says the HUD item is wanted |
| DamageDisplayer::DamageDisplayer / renderHUD | 0x140078c10 / 0x14007ae80 | 0x5a6da0 / 0x5a6dc0 | B / B\* | stores `this`; renderHUD hidden while short 0x26a4848 != 0, cell 0x2690b48 (cancel) |
| RotatingObjects::RotatingObjects | 0x1400ba560 | 0x69a680 | B | After the original removes entries whose node pointer is null from the 24-byte entry vector (+0x60..+0x68) |
| RotatingObjects::update | 0x1400bae20 | 0x69a720 | B | Feeds the original a speed factor `clamp((car+0x4ac - 100) * 0.01, 0, 1)` times dt (spinning parts like fans slow down below 100) |

### 2.5 Track and sky objects

| acs.exe symbol | acs VA | CSP RVA | Class | What changes |
|---|---|---|---|---|
| TrackAvatar::TrackAvatar | 0x1401c5250 | 0x1124da0 | B | wrapper 0x1124dd0 + post-construct 0x112a250 |
| TrackAvatar::init3D | 0x1401c8740 | 0x11258b0 | B | Before: pre-step 0x112ace0, load-counter byte +1 (0x26a6c9d). After: reads `graphics_adjustments.ini [FIXES] FIX_GROOVES_RENDERING_ORDER` and the track config `[BASIC] NO_NEED_FOR_FIXES`; if the fix is on: collects active class-0x0f meshes accepted by predicate 0x11253f0 (rejects: missing transparent flag, blend mode, too small, depth mode), groups by material (log: "N meshes, M materials, checking materials"), and moves the whole group behind a reference mesh (sets +0xe2 = 0, material extra +0x9c = 4, notifies parent). Sets flag 8 in every top-level node's extended data (+0x160) and raises global progress value 0x25e272c to >= 0.2 |
| TrackAvatar::addPhysicsMesh | 0x1401c78e0 | 0x1128990 | B\* | Scope "TrackAvatar::addPhysicsMesh <name>, adding physics mesh"; stores the mesh in global 0x26ad150; with `NEW_KN5_LOADER STRIP_PHYSICS_DATA` calls CSP's own adder (0x1128550, 1,083 B) instead of the original; warns once for a mesh offset larger than 3,000 ("scene origin shift would be welcome") |
| TrackAvatar::getSectorDescription | 0x1401c80c0 | 0x1127f80 | C | Walks the 40-byte sector list (+0x2f0) and returns the name for a normalised position, including sectors whose `from > to` (wrap through the start line) `[interp]` |
| TrackAvatar::initDynamicObjects | 0x1401c9840 | 0x1129970 | B | Sets 0x26a975d = 1; seeds the random number generator (cell function 0x26a36b8) with a hash of the track config list `[TWEAKS] DYNAMIC_TRACK_OBJECTS_SEED` (or time-based when absent), online: time scale 0 when `[DYNAMIC_OBJECTS] DISABLE_ONLINE_SYNC`, else sim time in s; original; then walks the dynamic object vector (+0x2d0, stride 0x38): a node named `baloon` / `FBX: baloon.FBX` becomes type 6 (+0x8c = 0x6000000), logs "Found a balloon object: ..." and sets 0x26a977c |
| TrackAvatar::initPhysics | 0x1401ca440 | 0x11274c0 | B | After the original: reads the track's `data/surfaces.ini` (`[_EXTENSION] PIT_ALTITUDE`, else `[BASE] ALTITUDE`), computes "Pit altitude for current track" and "Pit Y coordinate" from node `AC_PIT_0` ("Required to sync altitude AC_PIT_0 is missing!"), stores two floats (0x268c430 / 0x268c48c; default altitude 294 m when the value is unset) and raises progress to 0.25 |
| TrackAvatar::initPitCrew | 0x1401ca630 | 0x1129000 | B\* | cell 0x2697520 (cancel); then looks up the crew set named "Pit stop and crew" in the track's table (+0x240) and rotates it so the pit box of the car's pit index comes first before the original |
| TrackAvatar::initRespawnPositionSet | 0x1401cb8c0 | 0x11276a0 | B | Before the original, for set name S: if `AC_S_0` is missing creates 40 dummy nodes `AC_S_<i>` spaced +-2 m laterally; if the set exists, creates every missing index below 40 ("Creating a missing spawn point `AC_HOTLAP_START_1`" x21 in the log); for `PIT` with no pit boxes also creates a single pit node with a +-10,000 m area. Also registered as a lookup |
| TrackAvatar::processPhysicsNode | 0x1401cc5e0 | 0x1126aa0 | B\* | Root node: creates two hash sets (multithreaded loading, `POSTPONE_WALLS_INIT`) and a deferred job. Mesh nodes (class 0x0f, active) whose name is only digits: vertical mesh (0x1126890) -> logged "Preventing vertical mesh from registering as a physics surface" and skipped; else "A strange digit-only physical surface" and original |
| TrackAvatar::update | 0x1401cca10 | 0x1129830 | C | Replaces the original: per car syncs a track-object visibility byte; then the same dynamic-object (balloon) motion as updateDynamicObjects |
| TrackAvatar::updateDynamicObjects | 0x1401ccb30 | 0x1129400 | C | Balloon drift: for objects of type 6, pushes the position along the wind (cell 0x26974e8 answer smoothed with lag 0.995) scaled by `(1 - dist / max(200, speed)) * wind alignment * random(0.6..1.4)` and applies velocities; copies positions to the node (+0x38 / +0x40) |
| TrackAvatar::getSurfaceDescFromMeshName | 0x1401c8300 | lookup | D | used by CSP's physics-surface code |
| TrackObject::TrackObject | 0x1401cf1a0 | 0x112cd10 | B | wrapper 0x112cd40 + post-construct 0x112d6d0 |
| TrackObject::resetOrgMatrix | 0x1401cf6d0 | 0x112d120 | B | Original; resets the node's scale (vtable +0x70, 1.0); for physics-linked objects schedules a deferred task |
| TrackObject::update | 0x1401cf720 | 0x112d030 | B\* | With `[PHYSICS_EXPERIMENTS_DONE] CUSTOM_POBJECTS_MOTION` (flag 0x2695092): copies the physics object's matrix from the thread-shared 4-slot ring (`(+0x158 & 3) * 0x40`) into the node and updates it with `0xb8c6b0`, original skipped; else original then the same update |
| StartingLights::StartingLights | 0x1401a1a20 | 0x110a8c0 | B | stores `this` (0x26abf38), post-construct 0x110ac30 |
| StartingLights::renderLight | 0x1401a3420 | 0x110a900 | B | Replaces the light's alpha by `225 - ambient * 255` (lights dim with ambient brightness, 0x25e9814) |
| StartingLights::renderHUD | 0x1401a2e80 | 0x110a960 | B\* | Hidden while short 0x26ab048 != 0; temporarily clears a byte of the render target (+0x150 of +0x110) when 0x2697c0f; scales by the weather lighting factor |
| StartingTrackSemaphore::StartingTrackSemaphore | 0x1401a37a0 | 0x110ac50 | B | stores the semaphore node data (0x26975a0 = param+0x128), post-construct 0x110ad20; log "Starting light: x, y, z" / "Light pos for 1..5" come from here and 0x14ae010 / 0x14ae340 |
| IdealLine::IdealLine | 0x140109d10 | 0x6a2520 | B | wrapper 0x6a2550 + post-construct 0x6a2720 (log "IdealLine::IdealLine: 39815 us") |
| SunAnimator::SunAnimator | 0x1401ad890 | 0x1121d80 | B | post-construct 0x1122000 |
| SunAnimator::enableCustomSunDirection | 0x1401adab0 | 0x1121dc0 | B\* | Original skipped while WeatherFX controls the sun (global 0x26ae270 and predicate 0x3572b0); creates a CSP sun record |
| SunAnimator::setSunDirection | 0x1401adbd0 | 0x1121ec0 | B\* | Stores a sun pitch term (0x268... +0xc = 0.1 * value of 0x2691090+0x290) and the heading in CSP's sun record; original skipped under WeatherFX |
| SunAnimator::update | 0x1401adc10 | 0x1121f60 | B\* | cell 0x26974c8 (cancel); if a listener cancels, vanilla update (and its +-80 degree clamp) is not run |
| WeatherManager::WeatherManager | 0x1401d8020 | 0x112f960 | B | stores `this` (0x2697510), post-construct 0x112fa10 |
| WeatherManager::applyCustomWeather | 0x1401d8110 | 0x112f9a0 | B\* | With `DAT_18268b336` (CSP weather active): only copies the weather name into the manager (+0x60) and into the global string 0x25e29d0, clears 0x25e2a18, original not called |
| PostProcessEffectsUpdater::PostProcessEffectsUpdater | 0x1400ab880 | 0x6915a0 | B | stores `this` |
| PostProcessEffectsUpdater::update | 0x1400abc20 | 0x6915c0 | C | Replacement: for cars within 200 m (800 m with 0x25e981b) whose rank < 4 (10) and that are active, tests up to 10 effect zones (stride 0x18 vectors of 0x2c-byte entries, function 0x609f10) and writes per-zone flags (+0x8bc + zone * 0x1c) and a "any active" word (+0x86c) into the YEBIS parameter object |
| PostProcessEffectsUpdater::loadConfigurationFile | 0x1400abb70 | lookup | D | |

### 2.6 Mirrors (render side)

| acs.exe symbol | acs VA | CSP RVA | Class | What changes |
|---|---|---|---|---|
| MirrorTextureRenderer::MirrorTextureRenderer | 0x140113c40 | 0x1195780 | B | wrapper 0x1195800 + post-construct 0x1196920 |
| MirrorTextureRenderer::render | 0x140114410 | 0x1195df0 | C | Complete re-implementation of the mirror pass: FOV override (0x269512c or 0x26adac0 into +0x74), sets pass id 0x26a9e78 from 0x25e1178, flags 0x268b685 / b687, collects the car's mirror nodes (+0x120 vector of the HR / LR driver model) into the render list, then per mirror fires cells 0x2697770 / 0x2697778; handles `MIRROR_MIPS2` (0x2695148: mip generation of mirror targets, 0x1b0 vtable call); restores state; increments the mirror frame counter (+0x39) |
| VirtualMirrorRenderer::VirtualMirrorRenderer | 0x1401d1ab0 | 0x11cd160 | B | stores `this` (0x26adec0), post-construct 0x11cd3f0 |
| VirtualMirrorRenderer::renderVirtualMirror | 0x1401d1ea0 | 0x11cd1a0 | B\* | Stores frame counter, cell 0x2697ad0 (cancel); sets blend / sampler / render-target state (D3D11 context vtable 0x118, 0x158, 0x120) and calls the original |
| VirtualMirrorRenderer::update | 0x1401d2190 | 0x2db1f0 | C | **no-op** |

### 2.7 Where the content features live (not hooks; located by strings; `[dll]` function RVA (size))

The hook replacements are seams. The code that *implements* the new content features is in 1,331 ordinary functions
(707,061 bytes) between RVA 0x1440000 and 0x14f0000 (a contiguous block of "car / track / config" code `[interp]`) plus
a few siblings. Only strings were read, not bodies:

| Feature | Function RVA (size) | Strings that identify it |
|---|---|---|
| Config chain loader (car, per skin) | 0x14900d0 (3,785), decompiled | `ext_config.ini`, `csp_config.ini`, `sfp.ini`, `HEADER`, `SFP_TYRE_MODEL`, "Loading configs for ", ", skin: ", " ms, in total ", "[ConfigsCache]" in 0x1491f80 / 0x1492190 / 0x1492a90 |
| Track config loader | 0x13a5140 (4,796) | `extension\ext_config.ini`, `tracks`, `loaded`, `skins\default\cm_skins_active.json` |
| Car "extension" reader (the biggest) | 0x14818b0 (15,167) | `[BASIC] POSITION`, `GRAPHICS`, `_EXTENSION`, `_EXTENSION_DRIVER`, `LIGHT_HEADLIGHTS`, `LIGHT_HEADLIGHT_0`, `REAL_MIRROR`, `SMART_MIRROR`, `WIPERS`, `PARTICLES_FX`, `TYRES_FX`, `RAIN_FX`, `REFLECTIONS_FX`, `ADAPTIVE_EMISSIVE`, `AUDIO_TWEAKS`, `DIESEL_ENGINE`, `RALLY_CAR`, `OPEN_WHEELER`, `RACING_CAR`, `TURN_SIGNALS`, `DISABLE_LIGHTSINI`, `DISABLE_DIGITALINSTRUMENTSINI`, `DISABLE_ANALOGINSTRUMENTSINI`, `DISABLE_NEW_FLAMES_ENTIRELY`, `DOOR_ANIMATION_CLOSE_SKIP`, `STOP_LODS_ADJUSTMENT`, `HORN_AS_SIREN`, `HAZARDS_G_THRESHOLD`, `FLASH_HIGH_BEAMS`, `LAWNMOWER`, `KEEP_SEATBELT_WITHOUT_DRIVER`, `TILT_SEATBELT`, `MASK_CUBEMAP_*`, `RPM_VINTAGE_*`, `TRACES_LIGHTS_*`, `OVERHEAT_EFFECTS`, `SHARE_POSITION_DATA_FOR_NAVIGATORS` |
| Numbered section kinds of a car config | 0x1207c10 (1,869), 0x12073f0 (2,072) | ANIMATION, LIGHT, SCRIPT, ANIMATED_WIPER, ODOMETER, CAR_STEREO, NAVIGATOR, DYNAMIC_TEXTURE, LED_PANEL, SCRIPTABLE_DISPLAY, EMISSIVE, SECOND/MINUTE/HOUR_INDICATOR, TEXTURE_PREPROCESSING, EXTRA_SHAPE, EXTRA_FX, ANALOG_INDICATOR, RENDERING_CAMERA, DEFORMING, ADJUSTABLE_WING, BENDING_WIPER, SIDE_NET, MUDFLAP, FLAG_CLOTH, WOBBLY_MESH, WOBBLY_BIT, SHAKING_EXHAUST |
| Light sections | 0x1447970 (3,097) section table; 0x144f8b0 (10,993) headlights incl. pop-up (`POPUP_START/END/EDGE_*`, `SPOT_UP`, "Headlights are not found"); 0x14523b0 brake; 0x14574e0 reverse; 0x1458990 turn signals / cornering lamps; 0x1453ac0 KERS; 0x1454f70 open doors; 0x1455fd0 LED panel; 0x145d2c0 brake-disc lights; 0x145a080 licence-plate light (`LIGHT_LICENSEPLATE`, `LAYOUT`); 0x145eb50 / 0x14611e0 bounced and mirror bounced light; 0x1462290 "NEEDS_LIGHTS_FIX / NO_NEED_FOR_LIGHTS_FIX" (emissive compatibility); 0x14b2770 flame lights (`LIGHT_FLAMES`, `LIGHT_FLAME_n`); 0x14a9200 / 0x14a7320 light sources and Lighting FX settings | `LIGHT_*`, "Brake lights (index: %d)" ... |
| Condition inputs of lights / emissives | 0xc12db0 (26,041) | ~165 names (`SPEED_KMH`, `LOWBEAM`, `HIGHBEAM`, `BRAKE_LIGHTS`, `REVERSE_LIGHTS`, `HAZARD`, `TURNSIGNAL*`, `OPENDOORS`, `SEATBELT`, `WIPERS_MODE`, `WIPERS_PROGRESS`, `EXTRA_A..T`, `GEAR_*` ...) |
| Real mirrors | 0x1473ab0, 0x14748c0, 0x1475a10 (`REAL_MIRROR_n`), 0x14766b0 (6,427: `REAL_MIRRORS`, `ALTER_FOV`, `SHOW_CAR`, `SHOW_HEAD`, `FIX_VISOR`, `RENDER_PER_FRAME`, `VR_STEREO`), 0x146afd0 (`real_mirrors` folder), 0x146ba00 (mesh splitting), 0x146cf90 ("Mirror queue is not empty"), 0x14dd960 (virtual mirror availability), shaders `accVirtualMirror_ps.fxo`, `accVirtualMirror_hdr_ps.fxo` | `ext_mirror*` shader vars |
| Tyres FX | 0x14c8fa0 (10,923), 0x14c42e0, 0x14c7ac0 (`TYRES_FX_PATTERN`, `TYRES_FX_SPIKES_n`), 0x14c5c00, 0x14ccec0, 0x14cee90 (`BROKEN_TYRES_*`), 0x14cc8a0 ("TyresFX: flex points"), 0x14d37a0 ("TFX tyre profiles"), 0x14d4530 (`insTyreSpikes_*`) | `GRAPHICS_OFFSETS`, `FLEX_*`, `DIRT_*`, `DAMAGE_*`, `WEAR_MAX_VIRTUAL_VM`, `NEW_SHINE_*` |
| Exhaust flames / particles triggers | 0xb19410 (10,844: drivetrain `LIMITER`, `ANTILAG`, `FLAT_SHIFT`, `DISCHARGE`, `EXHAUST_FLAMES`), 0xdabbd0 / 0xda7c90 (`PARTICLES_FX_EXHAUST*`, rain occlusion heating), 0x1274360 (`__EXT_WHEEL_FLAMES`) | |
| Rain occlusion per car | 0xdabbd0 (10,782), 0xda7c90 (9,081) | `RAIN_FX_OCCLUSION*` |
| LOD sanity | 0x14a2800 ("Last LOD is way too detailed"), 0x14a21a0 ("Driver LOD B is way too detailed"), 0x149d5c0 (`COCKPIT_HR`, `STEER_HR`, `STEER_LR`, `GEO_MIRROR`), 0x14ec620 (`WHEEL_*`, `SUSP_*` nodes required) | |
| Driver model replacement | 0x14eabf0 (5,784) | `MODEL`, `HIDE_OBJECT_n`, `DRIVER:GEO_Driver_FACE`, `driver_no_HANS`, "Driver model `...` is missing" |
| Shadowed wheels / driver | 0x147fcb0 (5,215), 0x14873e0 (1,590) | `SHADOWED_WHEELS`, `SHADOWED_DRIVER`, `OPACITY`, `SHADOWED_TYRES` |
| Trees (billboards, baked) | 0xe6c1f0 (11,236) | `tree.ini`, `billboards/%u_*.dds`, "Tree manifest is missing" |
| VAO patches (track and car) | 0x11e0fa0 (4,257), 0x11e2050 (2,470), 0x1014f40 | `vao-patches`, `.vao-patch`, "Applying VAO patch ... to KN5: monaco_2017.kn5, class: Node [1]: 1125 meshes, 532.59 ms" |
| Track section kinds | 0x12e8c40 (1,403), 0x12e7ee0 (1,598), 0x139bc20 (662) | GAUSSIAN_SPLATS, AREA_HEIGHT_MAP, SURFACE_WAV_OVERRIDE, LIGHT, FAKE_SHADOW, AUDIO, SCRIPT, HEAT_SHIMMER, SHADER_REPLACEMENT, SCRIPTABLE_DISPLAY, EVENT, REFLECTIONS_FX, CONDITION, DISPLAY, TREES, SPECTATORS, EXTRA_FX, ANIMATED, MATERIAL_ADJUSTMENT, DEFORMING_WALLS, GARAGE_DOORS, LIGHTS_DISTANT_LAYER, MESH_ADJUSTMENT |
| Track `[BASIC]` / physics-adjacent keys | 0x139e4e0 (6,511) | `PITBOXES`, `RALLY_TRACK`, `SUPPORTS_WIND`, `SKY_BELOW_HORIZON`, `CLOUDS_OVER_SCENE`, `PUDDLES_AMOUNT`, `IDEAL_LINE_RAIN_FACTOR`, `FIREWORKS_POS`, `TRACK_OCCLUDERS`, `RANDOMIZED_OFFSET_*`, `LIMIT_CHUNKS_OPTIMIZATION`, `AUDIO_DISTANT_OCCLUSION`, `LIT_MULT`, `CAR_LIGHTS_LIT_MULT`, `REVERB_CONTRIBUTION` |
| Light pollution | 0x1342020 (80), 0x1342070 (79) | `LIGHT_POLLUTION` |
| Local cubemaps | 0xde9330 (4,092), 0xdf5090 | `LOCAL_CUBEMAPS_TRACKS2`, `REFRACTING_HEADLIGHTS2` |
| Track electronics / semaphore | 0x10cfbd0 (3,214: `AC_SEMAPHORE` kn5 parse), 0x14ad710 ("Starting lights light"), 0x14ae010 / 0x14ae340 ("Starting light: "), 0x14b1800 ("Creating car lights", "Creating track lights"), 0x14af5a0 (`light_points/%s.bin`) | |
| Ideal line | 0x139e4e0 neighbour 0x118cec0 (1,293: `IDEAL_LINE`) | |
| Scene optimiser | 0x14ed2e0 (1,550: "Fully static branch", "No renderable meshes", "Branch with AABB children"), 0x14ed8f0 (horizon tube) | seen in the log after the car |
| Extra lights hotkeys | 0x12535a0 (5,119) | `SWITCH_%c`, `__EXT_LIGHT_%c`, "Extra %c: %ls (%ls)" |

### 2.8 The load timeline in the log (what proves the order)

One offline race, `custom_shaders_patch.log` of the owner: track first (monaco_2017), then the car. `[log-csp]`
(timestamps in ms since start): `Loading configs for monaco_2017` 11,831 (cached) -> `Track KN5s start` -> inserted FBX
`monaco_2017__displights.fbx` (model replacement) -> VAO patch opened 13,211 and applied per kn5 (1125 meshes, 532 ms)
-> grooves fix 13,800 (22 meshes, 3 materials, moved behind `commis_b01`) -> surface mismatch warning
(`02OFFTRACK-PIT`) -> AI splines -> **pit altitude 8 / pit Y -13.3419** (13,937) -> `KN5 file is damaged or empty:
content/tracks/monaco_2017/12.kn5` -> IdealLine ctor 39.8 ms -> 21 missing hotlap spawn points -> `flag_colors.lut`
and `colorfulness.lut` not found -> trees -> static-branch analysis (1279 static meshes) -> horizon tube -> Embree
build. Car: aero LUT, "Using AC Patch Custom DWB Suspension", tyre compound loading (physics) -> `Loading configs for
rss_formula_2010, skin: 01_rss_red_7` 14,095 (225.56 ms, 1396 values) -> collider list -> Embree -> LOD A, B, C, D
(each: windscreen test per mesh, rain occlusion, mirror splitting) -> `Fake shadows for rss_formula_2010` -> car
bounds -> sound bank -> `shadow caster generated from last LOD, 14286 triangles` -> `Starting light:` x2 ->
`Headlights are not found` (the car has none) -> `Odometer is ready` -> `Loading car script: SCRIPTABLE_DISPLAY_0`,
`SCRIPT_0` -> `Loading is finished in 33.144 s` -> optional parts ("Candidate: wing 6, Skip: setup values do not fit")
-> scene tree traversal and chunk groups (`Chunks optimizer initialization: 12355.90 us`).

## 3. Config keys and Lua API

### 3.1 `general.ini` flags read by these hooks (shipped comment in my words; flag RVA)

| Flag RVA | Section / key | Default shipped | Read by | Meaning |
|---|---|---|---|---|
| 0x268bcb2 | `[PHYSICS_EXPERIMENTS] EQUALIZE_PITS_CHECK` | 1 | CarAvatar::isInPit | one pit-distance threshold (1.5 m) for every car |
| 0x268bcb1 | `[TWEAKS] LOAD_INACTIVE_SEATBELT` (hidden) | 0 (forced 1 by launch byte 0) | CarAvatar::init3D | also load the seatbelt-off model |
| 0x268bcb0 | `[CONTROL] UNBIND_TURBO_DIGITS` via Sim::onKeyDown | 0 | CarAvatar::setTurboBoost | digit keys no longer set turbo boost |
| 0x26950ab | `[OTHER_EXPERIMENTS] EXTRAPOLATE_STATE` | 4 (0 off, 1 basic, 2 not remote, 4 precise remote) | CarAvatar::setNewPhysicsState | extrapolate car and wheel positions between physics states |
| 0x26950a8 / a9 | `[BASIC] PREFER_DISABLED_ABS2` / `_TC2` | commented | CarAvatar::setAbsEnabled / setTcEnabled | keep ABS / TC off even if the server forces them |
| 0x268b43d | `[DEV] FLAMES_ALWAYS_ON` (hidden) | 0 | Flames::update | debug: flames permanently on |
| 0x269514d | `particles_fx.ini [FLAMES] ENABLED` (read only when `[OPTIMIZATIONS] PREVENT_CREATING_UNNECESSARY2`, hidden, commented default 1) | 1 | Flames::loadTextures | AC flames not created |
| 0x269514c | `particles_fx.ini [SMOKE] ENABLED` (same gate) | 1 | INIReader hook 0x5f0650 | AC tyre smoke not created |
| 0x2695188 / 89 / 190 | `[OPTIMIZATIONS_CPU] LIMIT_GENERAL2` [interp], `LIMIT_SHADOWS2` [interp], `LIMIT_SMOKE2` | commented (1 / 1 / 3) | many per-car updates, CarFakeShadow, TyreSmoke | skip work for far cars; smoke only for the nearest N |
| 0x269516b | `[OPTIMIZATIONS_MEMORY] ESTIMATE_MEMORY_CONSUMPTION` | 0 | CarAvatar::init3D / initPhysics / initCommonPostPhysics | memory scopes "Car #%d/Model / Physics / Extras" |
| 0x2695092 | `[PHYSICS_EXPERIMENTS_DONE] CUSTOM_POBJECTS_MOTION` (hidden) | 1 | TrackObject::update | physics object matrices taken straight from the physics ring |
| 0x2695069 | `[NEW_KN5_LOADER] STRIP_PHYSICS_DATA` | 1 | TrackAvatar::addPhysicsMesh | CSP's own physics mesh adder |
| 0x2695099 | `[PHYSICS_EXPERIMENTS] POSTPONE_WALLS_INIT` | 1 | TrackAvatar::processPhysicsNode | postpone wall set up |
| 0x2695038 | `[OPTIMIZATIONS] MULTITHREADED_LOADING` | 1 | TrackAvatar::processPhysicsNode | |
| 0x268b410 / b411 | `[PHYSICS_EXPERIMENTS] TWEAK_TRACK_OBJECTS` / `OPTIMIZE_COLLIDERS` | 1 / 1 | track object physics (other brief) | |
| 0x26950fc / fd | server `[EXTRA_RULES] ENFORCE_BACK_TO_PITS_PENALTY` / `_STOP` | online only | CarAvatar::goToSpawnPosition | read from the welcome message by `ACClient::onWelcomeMessageReceived` |
| 0x2695148 | `smart_mirror.ini [BASIC] MIRROR_MIPS2` (hidden, commented 1) | 1 | MirrorTextureRenderer::render | MIPs on rear-view mirror targets |
| 0x2695162 | neck head sway enable (`neck.ini`) [interp] | | DriverModel::updateHeadMovement | G-force head sway |

### 3.2 Car and track config sections these hooks (or the code behind them) read `[dll]`

* Per car (`extension/ext_config.ini`, `config/cars/{loaded,generic,common}`): `[BASIC]` (`LOAD_SEATBELT_OFF`,
  `POSITION`), `[GRAPHICS]`, `[_EXTENSION]`, `[_EXTENSION_DRIVER]`, `[LIGHT_*]` series (headlights with pop-up, brake,
  reverse, turn signals, cornering lamps, KERS, open doors, LED panel, licence plate, extra A..T, flame lights,
  brake-disc lights), `[EMISSIVE_*]`, `[REAL_MIRROR_n]`, `[SMART_MIRROR]`, `[WIPERS]`, `[ANIMATED_WIPER_n]`, `[PARTICLES_FX_EXHAUST*]`,
  `[TYRES_FX]`, `[RAIN_FX*]`, `[REFLECTIONS_FX]`, `[SHADOWED_WHEELS]`, `[SHADOWED_DRIVER]`, `[ADAPTIVE_EMISSIVE]`,
  `[AUDIO_TWEAKS]`, `[DRIVER]`/`MODEL` replacement, numbered kinds of 2.7. Details of the light / material
  language: `rendering_config.md` 3.5 - 3.7. On this PC: 12 installed cars use material / shader replacement, 15
  lights, 16 exhaust flames / particles, 12 tyre FX / brake disc, 13 animations / deforming, 7 mirrors,
  3 fake shadows (`content_features.json`).
* Per track (`extension/ext_config.ini`, `config/tracks/*`): `[BASIC] NO_NEED_FOR_FIXES`, `[DYNAMIC_OBJECTS]
  DISABLE_ONLINE_SYNC`, `[TWEAKS] DYNAMIC_TRACK_OBJECTS_SEED`, `[RAIN_FX]`, `[GRASS_FX]`, `[LIGHT_SERIES_n]`,
  `[CONDITION_n]`, `[MODEL_REPLACEMENT_n]` (a present `MODEL_REPLACEMENT_*` section sets flag +0x1169 of the
  config data), `[SHADER_REPLACEMENT_n]`, `[MESH_ADJUSTMENT_n]`, `[MATERIAL_ADJUSTMENT_n]`, `[DISPLAY_n]`,
  `[ANIMATED_n]`, `[LIGHT_POLLUTION]`, `[BOUNCED_LIGHT]`, `[TRACK_OCCLUDERS]`, ... (list in 2.7). On this PC 12
  tracks use material / shader replacement, 12 lights / light series, 11 rain FX, 12 track features
  (trees, wind, sky, conditions, occluders), 12 GrassFX (`content_features.json`).
* `data/surfaces.ini` of the track: `[_EXTENSION] PIT_ALTITUDE` and `[BASE] ALTITUDE` (read by TrackAvatar::initPhysics);
  `WAV_PITCH=extended-N` switches the extended physics (other brief).
* `data/brakes.ini [DISCS_GRAPHICS] LAG_HOT / LAG_COOL`, `data/damage.ini [SCRATCHES] MIN_SPEED / MAX_SPEED`,
  `graphics_adjustments.ini [FIXES] FIX_GROOVES_RENDERING_ORDER`.

### 3.3 Lua API that rides on these classes (substring counts over `lua_sdk_functions_union.txt`, 2,728 names)

| Class (hooks) | Lua names found |
|---|---|
| Scene access (all node / mesh work) | `ac.SceneReference`: **110 methods** in the `ac_apps` SDK (find / filter nodes, meshes, skinned meshes, `loadKN5`, `loadKN5LOD`, `createNode`, `createMesh`, `setMaterialProperty`, `setMaterialTexture`, `setAnimation`, `blendAnimation`, `setVisible`, `setShadows`, `createFakeShadow`, `applyShaderReplacements`, `projectTexture`, `projectShader`, `applySkin`, `raycast` ...); root selectors `carRoot:N`, `sceneRoot`, `carsRoot`, `trackRoot`, `staticRoot`, `dynamicRoot`, `luaRoot`; add parts to the node `BODYTR`; `ac.findNodes / findMeshes / findSkinnedMeshes / findAny` |
| Car state (CarAvatar, lights, driver, wipers) | `ac.StateCar`: 313 fields, among them `headlightsActive`, `brakeLightsActive`, `hazardLights`, `turningLeftLights`, `turningRightLights`, `turningLightsActivePhase`, `daytimeRunningLights`, `flashingLightsActive`, `hornActive`, `isDriverVisible`, `isDriverDoorOpen`, `isDriverSeatbeltOn`, `anyFlamesActive`, `wiperMode`, `wiperProgress`, `wiperSpeed`, `activeLOD`, `visibleIndex`, `isInPit`, `isInPitlane`, `pitTransform`, `driverEyesPosition`, `damage`, `dirt`, `suspensionDamage`, `isBlown` |
| Driver model (DriverModel) | `ac.replaceDriverModel`, `ac.packDriverModel`, `ac.updateDriverModel`, `ac.setCustomDriverModelSteerAngle`, `ac.setDriverDoorOpen`, `ac.setDriverEyesPosition`, `ac.setDriverMouthOpened`, `ac.setDriverVisible`, `ac.forceVisibleHeadNodes`, `ac.disableNeckFXDriverModelMovement`, `ac.disableDriverModelSync`, `ac.setDriverShiftAnimationPreloadThreshold`, `ac.getDriverHeadTransformTo` (37 names with "driver") |
| Lights / emissive | `ac.setHeadlights`, `ac.setAiHeadlights`, `ac.setAiHeadlightsSuggestion`, 8 emissive multipliers (`ac.setEmissiveMultiplier`, `setAdaptiveEmissiveMultiplier`, `setTrueEmissiveMultiplier`, `setEmissiveCameraGain`, `setReflectionEmissiveBoost`, ...), `ac.LightSource` (3 methods, 32 fields), `ac.setExtraSwitch` / `ac.getCarExtraSwitchState` / `ac.isExtraSwitchAvailable` (the A..T extra lights) |
| Wipers | `ac.setWiperMode`, `ac.setWiperSpeed` |
| Flames / smoke | `ac.Particles.Flame` (+ `emit`), `ac.Particles.Smoke`, `ac.setScriptFlamesIntensity`, `ac.overrideTyreSmoke`, `ac.setParticlesSmokeMaterial` |
| Mirrors | 15 names: `ac.RealMirrorParams`, `ac.getRealMirrorCount / Params / UV / AABB`, `ac.setRealMirrorParams`, `ac.pauseRealMirror`, `ac.disableVirtualMirror`, `ac.postponeVirtualMirror`, `ac.redirectVirtualMirror`, `ac.overrideVirtualMirrorShader`, `ui.drawVirtualMirror`, `display.mirror` |
| Damage / dirt / LOD | `ac.redirectDamageDisplayer`, `ac.getDamageLevel`, `ac.addSuspensionDamage`, `physics.setCarBodyDamage`, `ac.setBodyDirt`, `ac.forceDetailedCarLOD`, `ac.overrideLODMultiplier`, `ac.setExtraTrackLODMultiplier` |
| Fake shadows | `ac.SceneReference:createFakeShadow`, `setFakeShadowPoints / Squaredness / Opacity`, `ac.setWeatherFakeShadow*`, `GeometryShot:setFakeCarShadows` |
| Track painting / conditions | `ac.TrackPaint` (about 30 methods: `line`, `rect`, `arc`, `text`, `fill`, `textureStart / End` ...), `ac.TrackCondition(s)` (`get`, `getColor`, `input`, `isDynamic`, `name`), `ac.getTrackConfig`, `ac.adjustTrackVAO`, `ac.getTrackLightPollution` |
| Sky (SunAnimator / WeatherManager) | 21 "sun" names (`ac.setSunAngle`, `ac.setCustomSunDirection`, `ac.getSunDirection`, `ac.setSunColor`, ...), 16 "moon" names, 53 weather names (details in `rendering_config.md` 3.3 / 3.4) |
| Starting lights / ideal line / track objects | no Lua name found (`StartingLights`, `IdealLine`, `TrackObject`, balloons have no API); `physics.blockTeleportingToPits` and `ac.tryToTeleportToPits` belong to `CarAvatar::goToSpawnPosition` `[interp]` |

Environments: car scripts (`SCRIPT_n`, `SCRIPTABLE_DISPLAY_n`, physics scripts) run in `ac_car_*` environments, track
scripts in `ac_track_script`; the track script SDK has 1,797 functions, 1,002 of them its own (`lua_sdk_counts.tsv`).

## 4. Files it reads

* Car: `content/cars/<car>/extension/ext_config.ini` (+ `csp_config.ini`, `sfp.ini`), per-skin `skins/<skin>/...`
  (the loader names the skin), `extension/config/cars/{loaded,generic,common}`, `data/car.ini`, `lights.ini`,
  `lods.ini`, `brakes.ini`, `tyres.ini`, `suspensions.ini`, `drivetrain.ini`, `damage.ini`, `driver3d.ini`,
  `ai_default.ini`, `proview_nodes.ini`; `animations/lights.ksanim`, `animations/steer.ksanim`; LOD kn5 files,
  `collider.kn5`; `extension/vao-patches-cars/<car>.vao-patch`, `extension/real_mirrors/` (watched),
  `cache/csp_configs` (binary config cache), `cache/cars_data/<car>/rain_occlusion`, `cache/meshes_metadata`.
* Track: `content/tracks/<id>/extension/ext_config.ini`, `extension/config/tracks/{loaded,generic,common}`,
  `data/surfaces.ini`, kn5 models, `extension/vao-patches/<track>.vao-patch`, `ai/*.ai` (cached grids), `*.lut`
  (`flag_colors.lut`, `colorfulness.lut`), `light_points/<id>.bin`, `skins/default/cm_skins_active.json`.
* Globals: `Documents/Assetto Corsa/cfg/extension/general.ini`, `particles_fx.ini`, `smart_mirror.ini`, `neck.ini`,
  `graphics_adjustments.ini`, `race.ini` (assists), `cfg/video.ini`. `[dll][log-csp]`

## 5. How it is switched on

* The 115 detours are installed for every user; every gate is a run-time flag (`attach.md` 1.7). What an
  owner can switch off from outside: the flags of 3.1, `[BASIC] ENABLED=0` of each module ini, the
  per-car / per-track configs (no config = the hook still runs and calls the original: the 65 class-B hooks then
  behave like vanilla apart from the memory scopes and the guards).
* The four stubs of 1 (9) are unconditional.
* On this PC `[log-csp]`: modules on: Lighting FX, Weather FX (Lua style + controller), Smart Mirror (no-op flag,
  `REAL_MIRRORS=0` so CSP real mirrors are **off**), Tyres FX / brake discs (no-op flags), Skidmarks FX (new
  renderer "20000 particles"; skid data "faded to 0": skipped), track adjustments (grooves fix runs: 22 meshes).
  Off by owner override: Particles FX (`particles_fx.ini [BASIC] ENABLED=0`, so AC's own flames / smoke stay)
  and Windscreen FX; Extra FX default off. 5 extensions disabled by conditions (`attach.md` 1.5).

## 6. Size

| Item | Number |
|---|---|
| Rows | 140 (118 hooks + 22 lookups) of 30 classes in `hook_groups.tsv` group `rendering_hooks` without KunosSim / kgl / ksCameraYebis / ksParticleSystem / FW1 |
| Classes | B 65, B\* 33, C 20 (4 are the shared no-op 0x2db1f0), A 0, D 22 |
| Distinct replacement functions | 115 (114 real + 1 stub); `.pdata` bytes 43,846; pseudo-C 316,985 bytes |
| By class bytes | TrackAvatar 11,421, CarAvatar 8,581, BrakeDiscGraphics 4,488, TyreSmoke 3,920, DriverModel 2,425, VisualDamageManager 2,071, MirrorTextureRenderer 1,764 ... |
| Signal cells | 18 distinct (2.2) |
| Content code block | RVA 0x1440000..0x14f0000: 1,331 functions, 707,061 bytes; hook-replacement block 0x10a0000..0x1100000: 1,237 functions, 378,247 bytes |
| Config loader | `0x14900d0`, 3,785 B; log "in total 1396 values" for one car |
| Log numbers (this PC) | grooves fix 22 meshes / 3 materials; VAO patch monaco_2017 1125 meshes 532.59 ms; 21 created spawn points; LOD rain occlusion 0.49 - 1.11 s each |

## 7. Port difficulty for rustyAC

Overall **L** (content features) on top of an **S** seam layer. `[interp]`

* **What exists already.** `docs/port/renderer_car.md`, `renderer_scene.md`, `renderer_leftovers.md`,
  `damage_sparkles.md`, `tracks.md`, `draw_hear_cars.md` port the vanilla behaviours of these very classes
  (CarLodManager, DriverModel, BrakeDiscGraphics, Flames, TyreSmoke / TyreBlur / SkidMarkBuffer, CarFakeShadow,
  StartingLights, IdealLine, SunAnimator, mirrors, `TrackAvatar::initRespawnPositionSet` etc.), each with an
  oracle (`render_oracle`, `car_oracle`). Every CSP class-B hook is a pre / post step around an existing
  function, so the seam layer is small: **S**.
* **Cheap and provable (S)**: `isInPit` threshold, `EXTRAPOLATE_STATE`, seatbelt flag, loadLod guard + log, rank
  throttling, spawn-point synthesis (count 21 on monaco_2017 is a number to match), pit altitude (8 and -13.3419),
  digit-only / vertical physics mesh filter, sector wrap-around, RotatingObjects speed factor, DriverModel gear-name
  mapping. Proof: the CSP log lines above are exact oracles (run the Rust port headless on the same track and
  compare the printed numbers and the created node names).
* **Medium (M)**: grooves fix (needs the transparent-flag / blend / depth-mode predicate `0x11253f0` read), brake disc
  glow by shader name (the port's BrakeDiscGraphics becomes shader-based), TyreSmoke parameter overrides (read the
  rest of 0x11cc280), balloons and dynamic-object seeding, CarAnimations / AnimatedLights `lights.ksanim` rule,
  weather gate (needs WeatherFX).
* **Large (L-XL)**: the content features behind the seams: new lights (headlights with pop-up, 20 extras, condition
  language), flames (`LIGHT_FLAMES`, antilag / limiter triggers), real mirrors (`real_mirrors`, per-mirror
  render targets), Tyres FX (flex, spikes), GPU skid marks, fake-shadow baking, VAO patch format (Patch_v4 / v5
  not decoded), INIpp config language, car / track Lua environments. These are covered by the other briefs; in
  *this* area the biggest single unknowns are 0x14818b0 (15 KB) and 0x144f8b0 (11 KB).
* **Hard parts**: (1) the per-car extension object has no description (only offsets above); (2) signals hide the
  actual listeners; (3) 0x14900d0 contains an obfuscated key block (see 8); (4) real mirrors and skid marks need
  render passes the Rust renderer does not have; (5) online behaviours (`EXTRA_RULES`, input-method check).
* **What could be proven and how**: run `acs.exe` with CSP in the harness and with the same content under
  `render_oracle` / `car_oracle`; compare node lists after `init3D` (seatbelt nodes), the order of `Load LOD`,
  the created spawn points, pit altitude and the draw order of the 22 groove meshes; for sky gates compare
  sun angle past 80 degrees.

## 8. Open questions and not mapped

1. **Who listens to the 18 signal cells** and what the listeners do: not visible from the hooks (listeners are
   registered at module construction, `0x14dc880` family).
2. **Which `CarAvatar::CarAvatar` overload** each of the two hooks replaces (same VA in the table; atlas names the
   full car 0x1400cd1a0 and the net car 0x1400cddc0).
3. **`0x14900d0` encrypted / obfuscated block**: a 33-byte literal (0x462d37e95cf35c19 ...) is used to derive a
   path when a car's `sfp.ini [HEADER] SFP_TYRE_MODEL` is true; the loader then reads a protected file and
   injects its sections into the car config under `[internal]`. It guards a third-party (SFP) tyre model; per the
   rules it is **not decoded** here.
4. **`0x26a9d50` semantics**: load context vs. single-car debug override vs. "current car" (80+ physics hooks read it).
5. **Class names of flags** 0x268b415, 0x26a9186, 0x268b39e / b39f, 0x268b336, 0x26a9aaf, 0x25a2cd4 / 2cf0 (override
   values likely set from Lua `ac.setBodyDirt`, `physics.setCarBodyDamage`) are inferred, not proven.
6. **`Flames::loadFlames` closure 0x1098d50** and the global callback triple 0x2691150..60: probably an INIReader
   override that merges `flames.ini`; only the swap was seen.
7. **How wipers, reverse lights, turn signals, headlight projection, flex, deforming bodywork are wired**: they
   have no hook of their own in this class set (they are driven through `AnimatedLights`, `CarBrakeLights`,
   `AnimationPlayer`, `Material`, scene creation); only the strings of 2.7 are known.
8. **`VisualDamageManager::update` override and `DynamicCarEffects` global overrides**: purpose by name only.
9. **Balloon wind**: sign and units of the wind vector (cell 0x26974e8) and the 0.995 lag.
10. **StartingLights / IdealLine / TrackObject**: only wrappers were read; the features behind them (log "Starting
    light: ...", "Light pos for 1..5") were not decompiled.
11. The decompile of `TyreSmoke::update` beyond 420 lines of 516 (the tail repeats the override-and-restore pattern),
    `VisualDamageManager::initScratchableParts` beyond 230 of 311 lines and `CarMirrorManager::update` internals
    (`0x180367170`, `0x181182c40`) were skimmed.

Read in full: all 28 CarAvatar hooks (+ BackfireParams, BrakeBiasNotifier), TrackAvatar's 11 hooks, BrakeDiscGraphics,
CarNodeSorter::sort, DriverModel's 4, CarMirrorManager's 2, MirrorTextureRenderer::render, VirtualMirrorRenderer's 2,
all Flames / AnimatedLights / CarAnimations / CarBrakeLights / CarLodManager / TrackObject / StartingLights /
SunAnimator / WeatherManager wrappers, `decomp_req1/14900d0.c` and `1126df0.c`. Skimmed by strings only: everything in
2.7. Not read: any function not in `decomp_hooks/` or `decomp_req1/`.

## Evidence

* `hook_groups.tsv` (140 rows), `csp_hook_table.tsv`, `decomp_hooks/{10ab7a0,10abc20,10abdd0,10ac5d0,10aaf00,10aac80,
  10ac2b0,10abee0,10ae290,10ad200,...}.c` (all 115 files), `decomp_req1/14900d0.c`, `decomp_req1/1126df0.c`,
  `decomp_req1/138ded0.c`, `decomp_req1/1387350.c` (settings readers, flag names), `dll_func_strings.tsv`,
  `dll_strings.tsv` (RVAs 0x2216840.., 0x228b518.., 0x2287540, 0x22864b0), `dll_funcs.tsv`.
* `extension/config/general.ini`, `particles_fx.ini`, `smart_mirror.ini`, `neck.ini` (shipped comments), the
  owner's `Documents/Assetto Corsa/cfg/extension/particles_fx.ini` (`[BASIC] ENABLED=0`).
* `Documents/Assetto Corsa/logs/custom_shaders_patch.log` (lines 124-1130).
* `extension/internal/lua-sdk/ac_apps/lib.lua` (SceneReference, StateCar), `ac_track_script/README.md`,
  `lua_sdk_functions_union.txt`, `content_features.json`.
* `docs/atlas/content_loading.md` 3.2 - 3.4, `docs/port/renderer_car.md`, `damage_sparkles.md`; other briefs
  `attach.md` 1.7, `rendering_core.md` 2.8 - 2.10, `rendering_config.md` 3.4 - 3.7, `physics_car_hooks.md`.

## Decompile requests

1. 0x10ab7e0 (1,081 B): CarAvatar ctor wrapper 1: what exactly runs, which overload, the input-method rules.
2. 0x10af030: the post-construct that creates the per-car extension object; its field list.
3. 0x10ab3f0 (909 B) and 0x10ab2b0 (67 B): ctor 2 helpers.
4. 0x14818b0 (15,167 B): the car `_EXTENSION` reader: every key with default.
5. 0x1447970 (3,097 B): LIGHT_* section dispatcher; 0x144f8b0 (10,993 B) headlights and pop-up.
6. 0x1207c10 (1,869 B) and 0x12073f0 (2,072 B): numbered section kinds -> factories (wipers, wobbly bits,
   deforming, side net, mud flaps, flag cloth).
7. 0x14766b0 (6,427 B), 0x14748c0, 0x1475a10: real mirrors configuration and piece layout.
8. 0x146afd0 (2,008 B) and 0x146ba00 (2,314 B): mirror mesh splitting and UV normalisation.
9. 0x14c8fa0 (10,923 B): Tyres FX settings (flex, dirt, damage, broken tyres).
10. 0x14b2770 (8,146 B): flame lights; 0xb19410 (10,844 B): exhaust flame triggers.
11. 0xef1700 (98 B) and 0x180ed4fb0 (129 B): launch flag struct fields and the car-load context getter.
12. 0x1092290 (292 B), 0x1091970 (163 B), 0x1091b50 (108 B), 0x1094ad0 (106 B): CarLodManager helpers
    (default LOD list from car.ini / lods.ini).
13. 0x10e1a20 (1,207 B): "lights.ksanim is used by CSP animation" check; 0x10e2020, 0x10e2040.
14. 0x1098d50 and 0x1098fd0: Flames loader closure and texture helper.
15. 0x1128550 (1,083 B) and 0x11282a0: CSP's physics mesh adder.
16. 0x1126890 (517 B): the vertical-mesh predicate; 0x11253f0 (359 B): grooves candidate predicate; 0x112ace0 (397 B):
    pre-step of TrackAvatar::init3D.
17. 0x13a5140 (4,796 B): track config loader (includes, skins).
18. 0x139e4e0 (6,511 B): track `[BASIC]` and `PITBOXES` keys; 0x12e8c40 (1,403 B): track section kinds.
19. 0x10cfbd0 (3,214 B), 0x14ad710 (2,290 B), 0x14ae010, 0x14ae340: AC_SEMAPHORE parse and starting light placement.
20. 0x14eabf0 (5,784 B): driver model replacement and HIDE_OBJECT handling.
21. 0x10b0dd0 (161 B) and 0x10b1e80 (1,029 B): CarNodeSorter key function and the full sort.
22. 0x5c0400 and 0x5c05d0: DynamicCarEffects set-constant and mesh registration.
23. 0x1094f30 and 0x1183b00: SkidMarkBuffer render pre-check and size helper.
24. 0x1181af0 region (SunAnimator record at 0x2691090 +0x290): what the pitch term is; 0x3572b0 (WeatherFX
    "controls the sun" predicate).
25. 0x112fa10, 0x110ac30, 0x110ad20, 0x6a2550, 0x6a2720: post-constructors of WeatherManager, StartingLights,
    StartingTrackSemaphore, IdealLine (what CSP registers).
