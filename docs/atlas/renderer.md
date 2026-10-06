# Renderer (Direct3D 11, scene graph, post-processing, VR, particles, car visuals)

Source: `acs.exe` + `acs.pdb` through the local index (`re/decomp`, `re/types`, `tools/re_query.py`, `tools/disasm.py`),
plus a read-only look at `system/shaders`, `system/cfg` and `content/weather` in the game folder. Shallow atlas pass:
constructors, per-frame entry points and file names only. Nothing was modified. Raw notes: `re/scratch/atlas_render/`.
"Confirmed" = read in the pseudo-C / call index. "Interpretation" is marked as such.

## 1. What it is

The renderer is a classic forward renderer on Direct3D 11, built in three layers.
At the bottom, `kgl.lib` is a thin C-style wrapper (`kgl*` functions) around the D3D11 device, swap chain, state objects,
buffers, textures, render targets and shaders; nothing above it touches D3D directly except the VR and YEBIS code.
In the middle, `KunosSim.lib` holds the engine: `GraphicsManager` (current matrices, lights, render states, four system
constant buffers), a scene graph of `Node` / `Mesh` / `SkinnedMesh`, `Material` + `Shader`, and a family of camera classes
whose `render` method *is* the frame: shadow maps, reflection cube map, sky, opaque meshes, transparent meshes.
On top, game code in `acs.exe` adds one small `GameObject` per visual feature of a car (brake lights, tyre blur, glowing
discs, damage, flames, smoke, skid marks, mirrors ...) that edits nodes and material variables every frame.
Shaders are never compiled at run time: 72 pre-compiled HLSL pairs are loaded from `system/shaders/win/<name>_vs.fxo` /
`_ps.fxo` and their variables are discovered with `D3DReflect`.
Post-processing is done by the third-party **YEBIS 2** library (`ppfx_dx11_x64.lib`, about 680 kB of code), driven by
`CameraForwardYebis`; with post-processing off the plain `CameraForward` renders straight to the back buffer.
Oculus (libovr) and OpenVR each have their own camera subclass that renders the same passes once per eye, and triple
screen is a third subclass that renders three off-axis views.
Sun, sky colours, fog and clouds come from ini files (`lighting.ini`, `colorCurves.ini`, `content/weather/<name>/`).

## 2. Main classes

| Class | Source / library | Size | Role |
|---|---|---|---|
| `kgl*` free functions | kgl.obj / kgl.lib | - | D3D11 wrapper: device, swap chain, states, draw calls, GPU timer queries, DirectWrite fonts (via FW1FontWrapper) |
| `KGLShader`, `KGLTexture`, `KGLRenderTarget`, `KGLCubeMap`, `KGLVertexBuffer`, `KGLIndexBuffer`, `KGLCBuffer` | kgl.lib | 0x90 / 0x40 / 0x30 / 0x58 / 0x10 | Handles returned by `kglCreate*` |
| `RenderWindow` | KunosSim.lib | 0xf0 | Win32 window + input events on top of `kglOpenWindow` / `WndProc` (kglWindow.obj) |
| `GraphicsManager` | KunosSim.lib | 0x4e0 | Central state: view/projection/world matrices, `LightingSettings` (0xb4), `RenderState` (0x1f8), `cbCamera` / `cbLighting` / `cbShadowMaps` / `cbPerObject`, blend/depth/cull modes, shadow-map binding. Owns `ShaderManager`, `GLRenderer`, `GPUProfiler`, `PvsProcessor` |
| `GLRenderer` | KunosSim.lib | 0x110 | Immediate-mode helper (`begin` / `vertex3f` / `end`, quads, full-screen quad) used by HUD, mirror quad, debug lines |
| `ShaderManager`, `Shader`, `ShaderVariable`, `ShaderResource`, `CBuffer` | KunosSim.lib | 0x28 / 0xc8 / 0x40 / - / 0x28 | Shader cache by name, reflected variables and constant buffers |
| `Material`, `MaterialVar`, `MaterialResource`, `MaterialFilter`, `MaterialFilterSM`, `MaterialList` | KunosSim.lib | 0xb0 | Shader + per-material variable values + texture slots; filters swap the shader for the shadow pass |
| `Node`, `NodeEvent`, `NodeBoundingSphere`, `Renderable`, `Mesh`, `SkinnedMesh`, `Model`, `TextNode` | KunosSim.lib | 0xe0 / - / - / 0x108 / 0x168 / 0x188 / 0x100 | Scene graph. `NodeEvent` fires a callback when it is reached during traversal (used for custom drawing) |
| `PvsProcessor`, `BoundingFrustum`, `CameraMeshFilter` | KunosSim.lib | 0x9f030 | Collects draw calls of a pass, culls (frustum, distance/LOD, exclusion), sorts, renders. `CameraMeshFilter` selects Opaque or Transparent |
| `Camera` -> `CameraShadowMapped` -> `CameraForward` | KunosSim.lib | 0x210 / 0x2a0 / 0x788 | View + projection; 3 shadow cascades; the frame (`render`), motion blur, `CubeMapRenderer` (0x3e0), optional `HDR` (0xd8) |
| `CameraForwardYebis`, `YebisPP` | ksCameraYebis.lib | 0xe80 / 0x6a0 | Frame with YEBIS post-processing; `YebisPP` is the same wrapper for the VR cameras |
| `CameraTripleScreen` | KunosSim.lib | 0x10c0 | Subclass of `CameraForwardYebis`; overrides `renderPass` to draw three screens |
| `StereoCameraForward`, `StereoCameraVive` | KunosSim.lib | 0xa48 / 0x1418 | Oculus (libovr) and OpenVR cameras; `OculusManager` / `ViveManager` (0xa0) are the game objects that hold them |
| `CameraMirror`, `MirrorTextureRenderer`, `VirtualMirrorRenderer`, `CarMirrorManager` | KunosSim.lib / acs.exe | 0x218 / 0x68 / 0x78 / 0x78 | Rear-view mirror texture, the on-screen virtual mirror, and the car's mirror materials |
| `SkyBox`, `WeatherGenerator`, `WeatherManager`, `SunAnimator` | KunosSim.lib / acs.exe | 0x80 / - / 0x80 / 0x88 | Sky dome + billboard clouds, weather presets, moving sun |
| `ParticleSystem`, `StaticParticleSystem`, `ParticleGenerator` | ksParticleSystem.lib | 0x1b0 / 0x1b8 / 0xb0 | Billboard particles (shader `ksParticle`); used by `TyreSmoke`, `EngineSmoke`, and for track crowds |
| `TyreSmoke`, `EngineSmoke`, `Flames`, `SkidMarkBuffer`, `Sparks` | acs.exe | 0xa8 / 0x90 / 0x158 / 0x198 | Effects per car. `Sparks.obj` is an empty shell (7 boilerplate functions, never constructed) |
| Car visual helpers (see section 3) | acs.exe | 0x60-0x158 each | One `GameObject` per feature, all created by `CarAvatar` |
| `Font`, `StringBlitter3D` | KunosSim.lib | 0x20 / 0x78 | 2D text (DirectWrite) and textured-atlas text in 3D (`content/fonts/<name>.png` + `.txt`) |
| `ScreenCapturer`, `DebugVisualizer`, `ShapeRenderer`, `RayPicker`, `GPUProfiler` | acs.exe / KunosSim.lib | 0xc0 / 0x178 / 0x20 / 0x48 / 0x30 | Screenshots, debug text and lines, wireframe boxes, mouse picking (photo mode), GPU timings |
| `AnimationPlayer`, `Animation`, `AnimationBlender` | KunosSim.lib | 0x28 | Plays `.ksanim` node animations (suspension, doors, wings, lights) |

## 3. Entry points

**Creation.** `Game::Game` 0x140241880 creates `RenderWindow` 0x1401fd640 and `GraphicsManager::GraphicsManager`
0x140201620, which calls `kglInit` 0x1400184b0 -> `initDX11` 0x14001a590 -> `createDeviceAndSwapChain` 0x14001a1d0
(`D3D11CreateDeviceAndSwapChain`, feature level 11 with a 10.0 fallback that disables MSAA), `createDepthBuffer`
0x14001acc0, `initBlendStates` 0x14001aeb0, `initCullStates` 0x14001b0a0, then `initCBuffers` 0x140202d20,
`initSamplerStates` 0x140203180, `initRenderFlags` 0x140202f60. `Sim::Sim` 0x140192070 then builds the scene:
`Sim::initSceneGraph` 0x140199d70 (nodes `ROOT`, `BLURRED`, `UNBLURRED`, `TRACK`, `SKIDMARKS`, `CAR_SHADOWS`, `CARS`,
`BEFORE_CARS_NODE`, `PARTICLES_NODE`, `RENDER FINISHED`), `Sim::createCamera` 0x1401982e0, `Sim::initCubemaps`
0x1401997a0, `Sim::initStaticCubemap` 0x14019a2a0, `Sim::initHDRLevels` 0x140199b00, `SkyBox::SkyBox` 0x14021c790.

**Which camera class renders** is decided once in `Sim::createCamera` from `cfg/video.ini`: `[CAMERA] MODE` =
`OCULUS` -> `StereoCameraForward` 0x140211370 + `OculusManager` 0x1400a01c0; `OPENVR` -> `StereoCameraVive`
0x140220b80 + `ViveManager` 0x1401d6140; `TRIPLE` -> `CameraTripleScreen` 0x1402248e0; otherwise
`[POST_PROCESS] ENABLED` -> `CameraForwardYebis` 0x140025db0 (+ `PostProcessEffectsUpdater` 0x1400ab880), else plain
`CameraForward` 0x14021f230.

**One frame, in order** (confirmed; the order is hard-coded in these functions, there is no pass list or config):

1. `Game::onIdle` 0x140242730 (main thread, called from `Game::run` 0x140242eb0): `Game::update` 0x140243010 walks the
   `GameObject` tree; this is where every visual helper, particle system and camera runs its `update`.
2. `GraphicsManager::beginScene` 0x140202580: default states, bind back buffer, `updateLightingSetttings` 0x140205190.
3. `Game::render` 0x140242d50 -> `SimScreen::render` 0x140188610 -> `Sim::renderScene` 0x14019e570:
   `WorldMatrixTraverser::traverse` 0x14021abf0 (world matrices of the whole graph), then
   **mirror**: `MirrorTextureRenderer::render` 0x140114410 (only on-board or with the virtual mirror on) draws the scene
   with `CameraMirror::renderOpaque` 0x14021bd80 / `renderTransparent` 0x14021bf30 into a texture, then the main camera:
   `sceneCamera->render(blurredNode, unblurredNode, rootNode, dt)` (vtable slot +0x38).
4. `CameraForward::render` 0x14021fc40 / `CameraForwardYebis::render` 0x14002a450:
   - **shadow maps**: slot +0x28 `CameraShadowMapped::shadowMapPass` 0x14020d6a0, a loop of 3 cascades
     (`beginShadowMapPass` 0x14020c4f0, `createShadowMapMatrix` 0x14020c790, `MaterialFilterSM` swaps in `ksShadowGen*`);
   - **reflections**: `CubeMapRenderer::render` 0x14021edb0 re-renders some faces of the dynamic cube map
     (`[CUBEMAP] FACES_PER_FRAME`), from the `BLURRED` node;
   - bind target: `HDR::begin` 0x14022e0d0, the YEBIS MSAA targets, or the screen;
   - **main pass**: slot +0x30 `CameraShadowMapped::renderPass` 0x14020cf20 = **sky** (`SkyBox::render` 0x14021d0d0,
     depth off) -> **opaque** meshes -> **transparent** meshes (depth write off). Each sub-pass is
     `PvsProcessor::begin` 0x14022a8c0 / traversal / `PvsProcessor::end` 0x14022b120 (`doFrustumCulling` 0x14022abc0,
     `doDistanceAndLod` 0x14022a970, `doRenderCalls` 0x14022ac80). With motion blur on, `CameraForward::renderBlurred`
     0x14021fde0 is used instead: blurred nodes -> `solveBlur` 0x140220280 -> unblurred nodes -> transparent;
   - **particles, skid marks, flames, fake shadows** are ordinary nodes of the graph (`ParticleSystem::render`
     0x1402607e0, `SkidMarkBuffer::render` 0x1401905e0, `Flames::onNodeRenderEvent` 0x140103aa0,
     `CarFakeShadow::onNodeRenderEvent` 0x1400e22b0), so they are drawn inside the main pass in graph order
     (interpretation: mostly in the transparent sub-pass, by their materials' blend mode). The on-screen virtual
     mirror quad is also a `NodeEvent` callback (`VirtualMirrorRenderer::renderVirtualMirror` 0x1401d1ea0);
   - **post-process**: `CameraForwardYebis::updateYebisParameters` 0x140029e30, `renderApplyEffect` 0x14002a740 ->
     `applyPostProcessing` 0x140025c30 (YEBIS `ApplyEffects`), then an FXAA full-screen quad (`ksFXAA`).
5. **GUI**: `GraphicsManager::setScreenSpaceMode` 0x1402048e0, `ksgui::GUI::render`, `Game::renderHUD` 0x140242e30
   (every game object's `renderHUD`: apps, overlays, `ScreenCapturer::renderHUD` 0x1401631a0).
6. `GraphicsManager::endScene` 0x140202850 -> `kglSwapBuffers` 0x140019080 (`Present`). GPU timer labels along the way:
   `SHADOW_MAP`, `CUBEMAP`, `MAIN_PASS`, `TRANS_PASS` (or `BLUR_PASS`, `BLUR_SOLVE`, `UNBLUR_PASS`, `TRANSP_PASS`),
   `YEBIS`, `END_FRAME`.

**VR / triple.** `StereoCameraForward::render` 0x1402135c0 (`oculusRenderPass` 0x140213230 per eye, `finishRendering`
0x140212d10 submits) and `StereoCameraVive::render` 0x140223720 (`UpdateHMDMatrixPose` 0x140221c00, `finishRendering`
0x140223210 -> `IVRCompositor`) reuse `shadowMapPass` / `renderPass`. `CameraTripleScreen::renderPass` 0x140225180
calls `beginVirtualScreenPass` 0x140224e30 per screen.

**Shaders and materials.** `ShaderManager::getShader` 0x140227b40 -> `kglCreateShader` 0x140019c50 ->
`KGLShader::loadShaderBinary` 0x14001f0c0 (`readBlob` 0x14001ef50, `createVertexShader` 0x14001f490,
`createPixelShader` 0x14001f550, `reflectVars` 0x14001f5f0, `getInputLayout` 0x14001eb00). `Material::initShaderVars`
0x14020ac30 copies the reflected variables; `Material::apply` 0x14020a6e0 binds them per draw. kn5 materials are read
by `KN5IO::loadMaterialsBinary` 0x140216240 (counted in content_loading.md): shader name -> `Material::setShader`
0x14020b700, each property -> `Material::getVar` 0x14020ab30 + `ShaderVariable::set`, each texture slot -> texture.

**Car visual helpers** (all `GameObject`s; created in `CarAvatar::init3D` 0x1400d3b90, `initCommonPostPhysics`
0x1400d6190 or `onPostLoad` 0x1400d92b0; each has an `update` called from step 1):
`SuspensionAvatar::update` 0x1401b3840 (moves wheel/hub/disc nodes from the physics state), `SuspensionAnimator`
0x1401b0770 (`.ksanim` suspension arms), `SuspensionGraphics::update` 0x1401b3ca0 (generated link meshes),
`CarBrakeLights::update` 0x1400e0450, `BrakeDiscGraphics::update` 0x14005d930 (glow), `TyreBlur::update` 0x1401cfdb0,
`BlurredObjects::update` 0x1400c43b0 (rim swap by speed), `VisualDamageManager::update` 0x1401d56f0,
`DynamicCarEffects::update` 0x140091ac0 (dirt), `CarAnimations::update` 0x140062160 (wings, doors, shifting),
`AnimatedLights::update` 0x14005aef0, `RotatingObjects::update` 0x1400bae20, `GearShiftShake::update` 0x140104ca0
(shakes the `SHIFT_HD` lever node), `CarMirrorManager::update` 0x1400e6fc0, `TyreSmoke::update` 0x1401d0b80,
`EngineSmoke::update` 0x1400935e0, `Flames::update` 0x1401046a0, `SkidMarkBuffer::addSegment` 0x1401900c0,
`CarColliderRenderer::renderWireframe` 0x1400e0bb0, `CarNodeSorter::sort` 0x14006be30 (orders cars by distance).

**Sky and sun.** `RaceManager::initLighting` 0x14013a3d0 reads `race.ini [LIGHTING] SUN_ANGLE, TIME_MULT` and creates
`SunAnimator` 0x1401ad890; `SunAnimator::update` 0x1401adc10 moves the sun and calls `updateLightingSetttings`.
`WeatherManager::applyCustomWeather` 0x1401d8110 and `SkyBox::updateCloudsGeneration` 0x14021db00 load the weather.

**Screenshots.** F8 in `Sim::onKeyDown` 0x14019a940 -> `ScreenCapturer::takeScreen` 0x140163470 -> `saveScreen`
0x140163360 -> `kglSaveScreenCapture` 0x1400187b0 (`D3DX11SaveTextureToFile`). `ImageGeneratorCamera` /
`ImageGeneratorDLLManager` are remnants of a professional multi-channel "image generator" build: only boilerplate left.

## 4. What it reads from disk

| Path (as built in code) | Format | Read by |
|---|---|---|
| `system/shaders/win/<name>_vs.fxo`, `_ps.fxo`, `<name>_meta.ini` (`[METADATA] ALPHATEST, SKINNED, PARTICLE`) | compiled HLSL bytecode + ini; 72 shaders, 216 files | `ShaderManager::getShader`, `KGLShader::loadShaderBinary` |
| `system/cfg/graphics.ini` `[DX11]` (`ALLOW_UNSUPPORTED_DX10`, `MAXIMUM_FRAME_LATENCY`, `MIP_LOD_BIAS`, `SHADOW_MAP_BIAS_0..2`, `SKYBOX_REFLECTION_GAIN`) | ini | kgl, `GraphicsManager`, `SkyBox` |
| `cfg/video.ini` (Documents): `[CAMERA] MODE`, `[POST_PROCESS]`, `[CUBEMAP] SIZE, FACES_PER_FRAME, FARPLANE`, `[MIRROR] HQ, SIZE`, `[EFFECTS] SMOKE, RENDER_SMOKE_IN_MIRROR`, `WORLD_DETAIL` | ini | `Sim::createCamera`, `Sim::initCubemaps`, `MirrorTextureRenderer`, `TyreSmoke`, `EngineSmoke` |
| `system/cfg/lighting.ini` (`[LIGHT] LIGHT_HEIGHT`, `[HDR] MIN_EXPOSURE, MAX_EXPOSURE`), `system/cfg/colorCurves.ini` (`[HEADER]`, `SUN`, `SKY`, `AMBIENT`, `HORIZON`, `LOW`, `HIGH`, `ANGLE_GAMMA`, `HDR_OFF_MULT`), `system/cfg/hdr.ini` | ini | `CameraShadowMapped`, `Sim::initHDRLevels`, `GraphicsManager::loadLightingSettings` 0x1402042b0, `HDR::HDR` 0x14022d430 |
| `content/weather/<name>/weather.ini` (`[CLOUDS]`, `[FOG]`), `.../colorCurves.ini`; `content/texture/clouds/*.dds`; track `data/lighting.ini` | ini, dds | `WeatherManager`, `WeatherGenerator::loadPreset` 0x140227260, `SkyBox`, `TrackAvatar::TrackAvatar` 0x1401c5250 |
| `system/cfg/ppfilters/<set>.ini`, fallback Documents `cfg/ppfilters/` (sections `YEBIS`, `TONEMAPPING`, `AUTO_EXPOSURE`, `DOF`, `GLARE`, `GODRAYS`, `HEAT_SHIMMER`, `VIGNETTING`, `CHROMATIC_ABERRATION`, `ANTIALIAS`, `COLOR` ...) | ini | `CameraForwardYebis::readPPSetOptions` 0x140026150, `YebisPP::readPPSet` 0x14002cd60 |
| `cfg/oculus.ini`, `cfg/openvr.ini`, `cfg/triple_screen.ini` (Documents) | ini | VR and triple-screen cameras |
| `content/tracks/<t>/cubemap_model.kn5`, `content/objects3D/cubemap_model.kn5` | kn5 | `Sim::initStaticCubemap` |
| `system/cfg/tyre_smoke.ini`, `tyre_smoke_grass.ini`, `tyre_pieces_grass.ini`, `engine_smoke.ini`, `skidmarks.ini`; `content/texture/smoke_0.png`, `grass.png`, `skids.dds` | ini, png, dds | `TyreSmoke` 0x1401d0000, `EngineSmoke` 0x140092de0, `ParticleGenerator::loadINI` 0x140261550, `SkidMarkBuffer` 0x14018f2d0 |
| Car `data/`: `lights.ini`, `flames.ini`, `flame_presets.ini`, `mirrors.ini`, `blurred_objects.ini`, `brakes.ini [DISCS_GRAPHICS]`, `damage.ini`, `ambient_shadows.ini`, `extra_animations.ini`, `wing_animations.ini`, `suspension_graphics.ini`, `suspensions.ini [GRAPHICS_OFFSETS]` | ini (inside data.acd) | the car visual helpers |
| Car folder: `animations/*.ksanim`, `texture/flames/*.dds|png`, `body_shadow.png`, `tyre_<n>_shadow.png` | ksanim, dds, png | `CarAnimations`, `SuspensionAnimator`, `AnimatedLights`, `Flames`, `CarFakeShadow` |
| `content/fonts/*.ttf`, `content/fonts/<name>.png` + `.txt` | ttf, atlas | `kglInitFonts` 0x140018210, `StringBlitter3D` |
| Any texture (dds/png/jpg/bmp) | via `D3DX11CreateShaderResourceViewFromFile/Memory` | `KGLTexture::KGLTexture` 0x140023820 |
| `system/cfg/assetto_corsa.ini` (`[SCREENSHOT] FORMAT`, `[MIRRORS] FOV, FAR_PLANE`, `[DAMAGE] GLASS_THRESHOLD`) | ini | `ScreenCapturer` 0x140162560, `MirrorTextureRenderer` 0x140113c40, `VisualDamageManager` 0x1401d2890 |

Written: screenshots to `Documents\Assetto Corsa\screens\Screenshot_<car>_<track>_<date>.jpg|bmp|png`.

## 5. Size

`python tools/re_query.py size <stems>`: **1,162 hand-written functions, 479,151 code bytes, 100 object files**.

| Group | Object files counted | Functions | Code bytes |
|---|---|---|---|
| kgl.lib (all 9) | kgl, KGLCBuffer, KGLCubeMap, KGLIndexBuffer, KGLRenderTarget, KGLShader, KGLTexture, KGLVertexBuffer, kglWindow | 161 | 52,943 |
| KunosSim.lib engine (48) | GraphicsManager, GLRenderer, ShaderManager, Shader, ShaderVariable, ShaderResource, CBuffer, Material, MaterialFilter, MaterialFilterSM, MaterialList, Mesh, SkinnedMesh, Renderable, Node, NodeBoundingSphere, NodeEvent, Model, ModelBoundaries, RenderTarget, CubeMap, CubeMapRenderer, CameraForward, CameraShadowMapped, CameraMirror, CameraMeshFilter, HDR, SkyBox, Font, StringBlitter3D, TextNode, PvsProcessor, RayPicker, ShapeRenderer, ShapeBuilder, GPUProfiler, DynamicBuffer, IndexBuffer, MatrixStack, WorldMatrixTraverser, BoundingFrustum, WeatherGenerator, RenderWindow, Triangle, SplineStripBuilder, Animation, AnimationBlender, AnimationPlayer | 373 | 141,191 |
| VR + triple screen (5) | CameraTripleScreen, StereoCameraForward, StereoCameraForwardVive, OculusManager, ViveManager | 84 | 35,321 |
| Post-processing (3) | CameraForwardYebis, YebisPP, PostProcessEffectsUpdater | 55 | 57,012 |
| Particles and effects (8) | ParticleGenerator, ParticleSystem, StaticParticleSystem, TyreSmoke, Sparks, EngineSmoke, Flames, SkidMarkBuffer | 89 | 53,214 |
| Car visual helpers (20) | CarBrakeLights, CarFakeShadow, BlurredObjects, TyreBlur, BrakeDiscGraphics, SuspensionAnimator, SuspensionAvatar, SuspensionGraphics, SuspensionGraphicsGenerator, VisualDamageManager, AnimatedLights, CarAnimations, CarNodeSorter, CarMirrorManager, MirrorTextureRenderer, VirtualMirrorRenderer, DynamicCarEffects, CarColliderRenderer, RotatingObjects, GearShiftShake | 303 | 116,836 |
| Sky, screenshots, debug (7) | SunAnimator, WeatherManager, ScreenCapturer, ImageGeneratorCamera, ImageGeneratorDLLManager, DebugVisualizer, HighLevelGraphicsDebugger | 97 | 22,634 |

Not counted here: `Camera`, `CameraMouseControlBase`, `CameraFacing` and all game cameras (camera.md);
`KN5IO`, `ResourceStore`, `SceneGraphCloner`, `Texture`, `CarLodManager`, `DriverModel`, `CarAvatar` (content_loading.md);
`TripleScreenManager`, `PostProcessFilterSelector`, `PhotoMode`, `FormOpenVR` (SystemApps.lib, UI apps); `IdealLine`,
`TrackAvatar`; `Curve`, `Collisions`. Third-party code is outside the 7,453 hand-written functions and not counted:
`ppfx_dx11_x64.lib` (YEBIS, 679,687 bytes), `FW1FontWrapper.lib` (42,219), `libovr.lib` (8,057 shim), `openvr_api.dll`,
`d3dx11_43.dll`, `D3DCOMPILER_43.dll`.

## 6. Port difficulty: **XL**

- It is the largest area outside physics (about 480 kB of Kunos code) and it leans on three closed pieces: the YEBIS
  post-processing library, the 72 HLSL shaders that exist only as compiled `.fxo` bytecode, and D3DX11 texture loading.
- **Do not port kgl.** Replace it with `wgpu` (or `windows`-crate D3D11 if the original `.fxo` files must be used as-is).
  The `kgl*` function list is a good checklist of what the abstraction must offer: about 90 calls.
- **Shaders are the real problem.** Options: (a) stay on D3D11 and load the original `.fxo` directly (fastest, Windows
  only, `D3DReflect` gives variable names); (b) disassemble / decompile the bytecode to HLSL and translate to WGSL with
  `naga`; (c) rewrite the handful that matter (`ksPerPixel*`, `ksPerPixelMultiMap*`, `ksTyres`, `ksSky`, `ksShadowGen*`,
  `ksParticle`, `ksSkinnedMesh`, `ksGrass`, `ksTree`, `ksBrakeDisc`, `ksWindscreen`). Material variable names come from
  the kn5 files and the car helpers (`ksEmissive`, `blurLevel`, `dirtyLevel`, `glowLevel`, `damageZones` ...), so the
  replacement shaders must keep those names.
- **YEBIS cannot be ported** (no source). Replace with own tone mapping / bloom / DOF / FXAA; the ppfilter ini keys then
  become approximate. Visual match with the original will not be exact.
- Scene graph, materials, culling, shadow cascades, cube-map reflections, sky and particles are standard techniques:
  M each, but they must follow the kn5 node/material model, so a ready-made engine (`bevy`) fits poorly; a custom
  renderer on `wgpu` + `glam` is the realistic route. Text: `glyphon` / `fontdue` instead of FW1FontWrapper + DirectWrite.
  Images: `image` + `ddsfile` instead of D3DX11.
- VR: `openxr` crate replaces both libovr and OpenVR. Triple screen is only three off-axis projections.
- The 20 car helpers are small (S each) but numerous, and each needs its car ini plus physics state from `CarAvatar`.
- Depends on: main loop (`Game`, `GameObject` tree), content loading (kn5, textures, `CarAvatar`), config (ini reader),
  camera area, and `CarPhysicsState` snapshots from the physics thread. Nothing in physics depends on the renderer.

## 7. Open questions

- **Counting choices.** `kglWindow` and `RenderWindow` are counted here (main_loop.md leaves them to the renderer).
  `KN5IO`, `ResourceStore`, `SceneGraphCloner`, `Texture` are *not* counted here because content_loading.md already
  counts them. `Animation*`, `Triangle`, `SplineStripBuilder` (ideal-line strip mesh), `ModelBoundaries` and
  `GearShiftShake` were not assigned to anyone and are counted here. `CameraFacing` is counted in camera.md as assigned,
  although it is really a track crowd-billboard renderer built on `StaticParticleSystem`.
- **Non-YEBIS HDR path.** `HDR.obj` has only a constructor, destructor, `begin` and `reset`; no tone-map / resolve step
  was found and `CameraForward::render` calls nothing after `HDR::begin`. Either the resolve is inlined elsewhere or the
  classic HDR chain (`ksPostToneMap`, `ksPostBlurH/V`, `ksHighPass`) is dead when YEBIS is off. Not followed.
- **Where exactly particles land** (opaque vs transparent sub-pass, and `RENDER_SMOKE_IN_MIRROR`) was inferred from the
  graph layout, not traced through `PvsProcessor::prepareDrawCallsDefault` 0x14022b1c0.
- Sort key and state caching inside `PvsProcessor::doRenderCalls`, the shadow cascade split maths and the cube-map face
  scheduling were not read (shallow pass).
- `system/cfg/sparks.ini` exists but no code refers to it (`Sparks` is an empty class): sparks look unimplemented in
  this build.
- How `PostProcessEffectsUpdater::update` 0x1400abc20 maps car state (speed, damage?) to YEBIS parameters was not read.
- The exact vertex layouts (`MeshVertex`, skinned, particle) are fixed by `getInputLayout` and the `_meta.ini` flags; they
  were not written down here.
