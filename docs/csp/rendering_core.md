# Rendering: core hooks (scene graph, materials, kgl, post-processing)

CSP does not add a second renderer next to AC's. It **re-implements AC's own rendering classes in place**: about 180
`KunosSim.lib` / `kgl.lib` / `ksCameraYebis.lib` / `ksParticleSystem.lib` functions get a replacement, and almost
every hot one also gets a CSP "signal" (a vector of listeners) so that CSP's own modules (Extra FX, G-buffer, TAA,
upscalers, rain, windscreen, skid marks ...) can run before / after / instead of the original. The new material
system, the shader archive, the enlarged constant buffers, the flip-model swap chain and the post-processing
replacement all hang off these seams. This file covers the seams (hooks) and what is visible from them; the
CSP-own effect modules that sit behind the signals are only mapped by RVA and string (section 8).

Class legend used below (the lead's A-D): **A** pass-through (calls the original unchanged, or jumps to it),
**B** wraps (pre / post work, or a signal, then the original), **C** replaces (the original is not called, or only
a helper of it), **D** reads (lookup: CSP only needs the address or the data symbol). `[interp]` = my inference.

## 1. What it does

Behaviour in plain words (all `[dll]` from the replacement functions in `decomp_hooks/`, unless marked):

1. **One signal per hooked function.** Replacement functions like `Camera::renderCamera`, `CameraShadowMapped::
   shadowMapPass`, `GraphicsManager::setViewport` ... each keep a heap vector of 40-byte records `{?, callback, ...}`
   (same record size as the registry of attach.md 1.7) and walk it **from the last element to the first**, handing the
   callback a small argument block. For "cancelable" signals the argument block carries a byte that a listener can
   set; the replacement then returns without calling the original. 35 signal cells were found in 30 of the 178
   distinct replacement functions of this area (table in 2.9; 116 cells in all 960 hook decompiles, a regex lower
   bound). Subscribers are CSP's own modules; nothing is exposed to content directly. `[dll]`
2. **AC's render-state cache is re-implemented.** `setBlendMode`, `setCullMode`, `setDepthMode`, `setSamplerState`,
   `setShader`, `beginScene`, `clearRenderTarget`, `setWorldMatrix` call the D3D11 immediate context directly through
   its vtable (offsets 0x48 `PSSetShader` ... 0x228 `CSSetShader`) with CSP's own "current shader / current layout /
   current topology" cache (globals at dll RVA 0x25e2a30..0x25e9008). The original bodies are never called. `[dll]`
3. **Pass-aware multi-variant shaders.** A `KGLShader` object gets a 16-bit slot (+0x1a) that indexes CSP's table
   of "shader sets" (pointer at dll RVA 0x2697428). `GraphicsManager::setShader` and `Shader::apply` pick the
   variant (VS / PS / input layout / optional HS+DS) by a global **pass id** (u16 at RVA 0x26a9e78): ids 1..4 are
   produced by `setBlendMode` from the blend mode (bit 0: blend mode is in the "opaque-like" mask 0x2A0C825) and by
   the current group (1-2 or 3-4), 0xD is the reflection cubemap pass, 0 is default. This is how one AC material is
   drawn by the main pass, the G-buffer pass, the wet-road pass and the shadow pass without AC knowing. `[dll]`,
   group meaning of 1-2 vs 3-4 `[interp]`.
4. **Shaders come from CSP, not from `system/shaders/win`.** `KGLShader::loadShaderBinary` recognises the wide
   prefixes `extension/shaders/main_fx/` and `extension/shaders/custom_nv/`, builds the file names `<name>_vs.fxo` /
   `<name>_ps.fxo` (+ `meta/<name>_meta.ini`, sections `METADATA`, `ALPHATEST`, `SKINNED`, `PARTICLE`,
   `TESSELLATION`, `TESSELLATION_DYNAMIC`, `DAMAGEDIRT`, `DISTANT_EMISSIVE`), reads them from CSP's virtual file layer
   (embedded zip #1 in `dwrite.dll`: 6,683 precompiled DXBC files with hashed names, 30,879,898 bytes deflated,
   123,161,141 bytes raw), creates D3D blobs (`D3DCreateBlob`) and calls AC's own `createVertexShader` /
   `createPixelShader`. When a shader is missing it logs "CSP shader is not found: " and falls back to the original
   loader. `ShaderManager::getShader` additionally resolves `ksPerPixel*` names (with a `_bending` variant) first
   against `main_fx/`, then `system/shaders/win/`, and prints "Shader not found: `". `extension/shaders.zip` on disk
   is a 172-byte empty placeholder ("Actual content have been moved to dwrite.dll"). `[dll][listing]`
5. **Constant buffers are enlarged.** `CBuffer::init` rewrites the size AC asks for, by buffer type 0..3 (type
   numbers = AC's commit slots: camera 0, per-object 1, lighting 2, shadow maps 3, see `docs/port/renderer_core.md`):
   type 1 size 0x40 -> 0x50 (and a second per-object block of 0x90 is created and kept as a global), type 0 size
   0xE0 -> 0x140, type 2 size 0xA0 -> 0x210, type 3 size 0xD0 -> 0xF0. `CBuffer::commit` uploads extra mirrored
   copies (three CSP-global buffer objects). CSP shaders read the extra bytes. `[dll]`; which AC struct each type is
   `[interp]` (slot numbering from the port doc).
6. **Origin shift** (`graphics_adjustments.ini [ORIGIN_SHIFT]`): a vec3 at dll RVA 0x25e2f08..0x25e2f10 is added to
   the camera position (`CameraForward::render`, `CameraForwardYebis::render`, `Camera::getViewMatrix`,
   `createShadowMapMatrix`), to every node world translation (`GraphicsManager::setWorldMatrix`, `WorldMatrixTraverser::
   traverse`), to YEBIS particle positions (`setupPfxParticles`) and the cubemap eye. `[dll]`
7. **Pixel density / render scale and upscaling start at render-target creation.** `KGLRenderTarget::KGLRenderTarget`
   (0x6f5b80) and `RenderTarget::RenderTarget` (0x119fc80) first call an installed callback that may change width,
   height, format and flags (callback cells at RVA 0x26914d8 and 0x26977b8); `CameraForwardYebis::initRenderTargets`
   and `YebisPP::YebisPP` scale the sizes with a global scale struct (RVA 0x25a29c8, factor at 0x25a29d4), and the
   MSAA custom resolve (`accCopyResolve_msaa{2,4,8}_ps`) + the "proxied output" path (0x117d670, strings
   "Resolutions match, direct output" / "Resolutions do not match, proxied output") bring the scaled scene back to the
   window. `[dll]`
8. **The post-processing step can be taken over.** `CameraForwardYebis::applyPostProcessing` (0x10a0ff0) tests CSP's
   own post-processing object (pointer cell RVA 0x26ab000/0x26ab008): if it exists CSP runs its pipeline
   (0x10a1330) instead of YEBIS' `ApplyEffects`; if it is null the first time it logs "PFX is null!" and calls the
   original. `YebisPP::applyPostProcessing` (0x11cdc50) does the same for the VR / triple camera wrapper
   (CSP routine 0x11cdd20). `[dll]`
9. **AC's FXAA quad becomes the anti-aliasing / upscaling hook.** `GLRenderer::fullScreenQuad` (0x10b9780) receives the
   shader of the quad; once the YEBIS camera exists (cell RVA 0x26ab020) it looks up the shader `ksFXAA` by name
   (cached at RVA 0x26bd408) and remembers a second object read from the YEBIS camera at +0x818 (cached at
   0x26bd400; what that is, is not documented: `[interp]` another full-screen shader of that camera). For the
   `ksFXAA` quad it opens the profiler zone "Post-processing anti-aliasing", fires the signal 0x2697258 with the
   camera and, if a listener set the "handled" byte, runs a CSP post step (0x10ba000) and **skips the AC FXAA draw**;
   for the other shader it fires the cancelable signal 0x2697268. The PPAA selector at 0x1228e20 logs "PPAA: using
   CMAA / FXAA 3.11 / custom FXAA / SMAA / MLAA / original FXAA shader / skipping original FXAA shader". `[dll]`;
   that 0x1228e20 is a listener of those signals `[interp]` (no direct call edge seen). This is also why
   `[ANTIALIASING] FORCE_FXAA2`-style behaviour ("enables FXAA in AC video settings without actually applying it")
   exists: AC must draw the FXAA quad for CSP to get the hook. `[ini]`
10. **Shadows**: `shadowMapPass` is wrapped (profiler zones "Sun shadows" and "Alt. shadows set"; a re-entrancy
    guard byte at camera+0x3E0), `createShadowMapMatrix` and `setShadowMapsSplits` are cancelable signals,
    `setShadowMapTexture` is reimplemented (it fills the shadow cbuffer straight from the camera's matrices and
    supports a 4th cascade when the cascade count global is 4), `setShadowMapMatrix` is replaced by a no-op. `[dll]`
11. **Textures and models**: `ResourceStore` gets a thread-safe path -> texture cache (spin-locked map at RVA
    0x26aba78 area) and loads through CSP's own D3DX/WIC/DDS loader; `getTextureFromBuffer` fires a signal first;
    `KN5IO::loadTexture` is reimplemented around that cache; `KN5IO::load` synthesises an empty
    `content/cars/nul/collider.kn5` node tree for cars without one and wraps the load in a log line "KN5: ...";
    `loadMaterialsBinary` is wrapped to register each loaded material; `Mesh::compile` can defer the GPU buffer
    creation to CSP's loading threadpool; `Mesh::render`, `Node::render` and `StaticParticleSystem::render` are
    replaced. `[dll]`
12. **Loading-time pump**: `CBuffer::init`, `KN5IO::loadTexture`, `KN5IO::getSkinOverridenTexturePath`,
    `Material::createCBuffers` and `Mesh::compile` all start with the same test (dev flag byte at RVA 0x25e2728,
    "is main thread" compare with the id at 0x25e27b8, a `GetTickCount64` period at 0x25e2780) and then call one CSP
    routine (0x0B503B0) with the elapsed seconds: the loading splash screen / window is kept alive from inside AC's
    blocking loader. `[dll]`; purpose `[interp]` (log lines "Looking for background", "Cropping outline").
13. **Fonts**: `kglInitFonts` is replaced by the shared no-op stub (0x2db1f0), `kglCreateFont` (two overloads) and
    `FW1FontWrapper::CFW1Factory::CreateFontWrapper` are re-implemented with a CSP glyph atlas
    (`[CUSTOM_FONTS_RENDERING]` keys), `Font::blitString` fires a (single-callback) signal and nudges the y of the
    size-50 / mode-2 text. `[dll][ini]`

## 2. Game parts it replaces or extends

Numbers: rows of `hook_groups.tsv` group `rendering_hooks` whose library is one of the five of this file: **228**
(183 hook + 45 lookup): KunosSim.lib 146 + 37, kgl.lib 10 + 3, ksCameraYebis.lib 19 + 3, ksParticleSystem.lib 6 + 2,
FW1FontWrapper.lib 2 + 0. They map to **178 distinct replacement RVAs** (427,884 bytes of pseudo-C). The shared
no-op stub `0x2db1f0` (a 1-byte `ret`) is the replacement of 6 rows here (`CameraShadowMapped::~CameraShadowMapped`,
`DebugLog::~DebugLog`, `GraphicsManager::setShadowMapMatrix`, `GraphicsManager::~GraphicsManager`,
`ResourceStore::~ResourceStore`, `KGLYebisParticleSet::update`) and of `kglInitFonts`; the same stub is used by 30 rows
of other groups. Destructors that are not stubbed (`Material::~Material`, `Mesh::~Mesh`, `CameraForward::~CameraForward`,
`Font::cleanup`) test the fast-exit flag byte at RVA 0x268b39b and skip destruction at shutdown. `[dll]`

The remaining 140 rows of the group are `.obj` classes of cars / tracks (CarAvatar 46, TrackAvatar 13, Flames 8,
CarLodManager 5, ...); they belong to the scene / car brief and are **not** analysed here. The 118 CSP-own names
(`ac_ext::*` 89, `efx::*` 8, `pp::*` 6, `dx::*` 4 ...) are in group `misc_core` and are covered in 2.10.

acs.exe addresses are VAs (`0x14....`); the CSP column is the replacement RVA in `dwrite.dll` (image base
`0x180000000`).

### 2.1 GraphicsManager (22 hooks, 14 lookups; `KunosSim.lib`, `GraphicsManager.obj`)

| acs.exe method (VA) | CSP RVA | Class | What CSP changes |
|---|---|---|---|
| `GraphicsManager` ctor (0x140201620) | 0x117e8a0 | B | original, then CSP's extension init (0x1181180) on the object; singleton cell at RVA 0x26ad4e8 is the manager everywhere in CSP |
| `~GraphicsManager` (0x1402020d0) | 0x2db1f0 | C | no-op (exit speed) |
| `beginMainRenderPass` (0x140202570) | 0x117eb20 | C | sets "in main pass" bytes (RVA 0x268b685, 0x268b413) and GM+0x70; these byte globals are what every other render hook tests |
| `endMainRenderPass` (0x140202840) | 0x117eb40 | C | CSP flush (0x11cd360), clears the bytes, then (unless post-processing is on and not in the final stage) fires signal 0x2697558 |
| `beginScene` (0x140202580) | 0x117f2d0 | C | reimplements the frame start: clears the cached shader stages with `VSSetShader/PSSetShader/GSSetShader/HSSetShader/DSSetShader/CSSetShader(0)` when marked dirty, resets blend / raster / depth / sampler state, binds the back buffer (`OMSetRenderTargets`), `setViewport`, calls the original `updateLightingSetttings`, fires 0x2697570 |
| `endScene` (0x140202850) | 0x117ca90 | A | jumps to the original |
| `clearRenderTarget` (0x140202640) | 0x117ca00 | B | skipped by a flag (RVA 0x2695134); AC's magenta debug clear colour (1,0,1) is replaced by (0.2,0.2,0.2,1) to hide holes in track models (`BLOCK_COLOR_CLEAR` family) |
| `commitShaderChanges` (0x1402026a0) | 0x117ec70 | C | commits AC's three system cbuffers (+0x408, +0x458, +0x480) and CSP's extra mirrored copies |
| `compile` (0x1402026e0) | 0x117f6b0 | B | cancelable signal 0x2697550 before the original (CSP shader module may take over setup) |
| `getLDRScale` (0x140202bb0) | 0x414fc0 | C | returns 1.0 (AC's LDR scale; matches the hidden `DISABLE_LDR_FIX2` idea) |
| `overrideSamplerState` (0x140204410) | 0x117c940 | C | `PSSetSamplers(5, 1, GM+0x3f8 or +0x400)` |
| `setSamplerState` (0x140204800) | 0x117c970 | C | `PSSetSamplers(0, 6, GM+0x3d0 ...)` |
| `setBlendMode` (0x1402044e0) | 0x117c7b0 | C | state cache + pass id update (see 1.3); `OMSetBlendState` |
| `setCullMode` (0x140204510) | 0x117c870 | C | `RSSetState`, honours double-sided override byte GM+0x378 |
| `setDepthMode` (0x140204580) | 0x117c8e0 | C | `OMSetDepthStencilState` from a 8-entry table |
| `setShader` (0x140204a00) | 0x117c9e0 | C | variant selection by pass id (1.3), input layout, topology (4 or 0x23 = 35 for tessellated), HS/DS |
| `setShadowMapBias` (0x140204a40) | 0x117ebd0 | B | multiplies the three biases by a factor >= 1 when the shadow map is below 512 px, then the original |
| `setShadowMapMatrix` (0x140204af0) | 0x2db1f0 | C | no-op (matrices are copied by `setShadowMapTexture`) |
| `setShadowMapTexture` (0x140204bf0) | 0x117ca60 | C | binds the shadow SRV at slot 14 (PS and VS cache), fills the shadow cbuffer with up to 4 cascades, the depth-bias terms and the inverse scales |
| `setViewport` (0x140204ff0) | 0x117f1b0 | B | stores the rect in CSP globals (RVA 0x26ad590..), signal 0x2697560, then the original |
| `setWorldMatrix` (0x1402050a0) | 0x117ef00 | C | adds the origin-shift vec3 to the translation, then CSP's matrix-stack push (0x117ece0) |
| `updateLightingSetttings` (0x140205190) | 0x117f630 | B | original, then sets GM+0x478 dirty and fires 0x2697570 with the lighting struct pointer (GM+0x470) |
| lookups (D, 14) | - | D | `clearRenderTargetDepth` 0x140202650, `clearTextureSlot` 0x140202660, `createGLRenderer` 0x140202710, `getProjectionMatrix` 0x140202c30, `getViewMatrix` 0x140202cc0, `getWorldMatrix` 0x140202cf0, `onResize` 0x140204330, `resetRenderStates` 0x140204440, `setProjectionMatrix` 0x140204660, `setRenderTarget` 0x140204760, `setScreenRenderTargets` 0x140204880, `setScreenSpaceMode` 0x1402048e0, `setTextureRT` 0x140204cd0, `setViewMatrix` 0x140204d00 |

Phases (what the GraphicsManager hooks cover): construction, frame begin (`beginScene`), main-pass bracket
(`begin/endMainRenderPass`), per-draw state (blend, cull, depth, sampler, shader, world matrix, viewport), per-frame
lighting (`updateLightingSetttings`), shadow binding (`setShadowMap*`), and shutdown. `endScene` is untouched; the
actual `Present` is replaced one layer down (2.7, `kglSwapBuffers`).

### 2.2 Constant buffers, kgl handles, shader objects, materials

| Class / method (acs.exe VA) | CSP RVA | Class | What CSP changes |
|---|---|---|---|
| `CBuffer::CBuffer` x2 (0x140219890) | 0x10b3470, 0x10b3500 | C | field setup + CSP init (0x10b47d0 / 0x10b41f0); flag word 0x10100 |
| `CBuffer::init` (0x140219990) | 0x10b41f0 | C | the size rewrite of 1.5; also records CSP's global pointers: RVA 0x2696890 = the per-object buffer (0x50), 0x26968a0 = a second per-object block created by a recursive init with size 0x90 (flag +8 set), 0x26968a8 = the camera buffer (0x140; its data pointer is cached at 0x26968b0) |
| `CBuffer::map` (0x1402199f0) | 0x10b3e30 | C | change detection by checksum (0x10b3d00) before uploading; averages two lanes when the packed lane-pair byte at +0xb is set |
| `CBuffer::commit` (0x140219920) | 0x10b4190 | C | thunk to 0x10b3ea0: uploads to the primary buffer and to up to three mirrored copies |
| `CBuffer::release` / `~CBuffer` (0x140219a10) | 0x10b3680 | C | frees CSP's shadow copy with `HeapFree` |
| `CBuffer::get/set` (0x140219970 / 0x140219a50) | - | D | looked up (CSP calls them) |
| `KGLCBuffer::KGLCBuffer` (0x140023cb0) | 0x10b36d0 | C | creates the D3D buffer through CSP's device wrapper (dynamic, constant-buffer bind, 0x10000 flag) |
| `KGLVertexBuffer` (0x140023d40) / `KGLIndexBuffer` (0x140023f60) | 0x10bed60 / 0x10bef10 | C | CSP's buffer creation path (0x107d270 / 0x0e7fdb0), supports initial data |
| `KGLRenderTarget::KGLRenderTarget` (0x1400232d0) | 0x6f5b80 | B | size / format callback (cell RVA 0x26914d8) then the original (slot 0x26a9318) |
| `RenderTarget::RenderTarget` (0x14021c030) | 0x119fc80 | B | same callback (cell RVA 0x26977b8); with the MSAA-in-use byte set, a 1-sample colour target with depth is promoted to 2 samples; then the original and CSP's post-init |
| `KGLTexture` ctor x2 (0x140023820, overloads 1+2) | 0x10bb280, 0x10bbac0 | B | original is wrapped by CSP's own creation (0x10bb2b0 / 0x10bbaf0, undecompiled) + registers the texture in CSP's tables (0x10bdbf0); string "Loading from held view" belongs to this family |
| `KGLTexture::initSize` (0x140023c10) | 0x10bbf20 | A | null-guarded jump to the original |
| `KGLShader::loadShaderBinary` (0x14001f0c0) | 0x11026a0 | B | CSP shader archive / meta ini (1.4); fall back through slot 0x268c330 |
| `KGLShader::reflectVars` (0x14001f5f0) | 0x1102350 | C | takes the blob via two vtable calls and runs CSP's reflection (0x11011c0) |
| `KGLShader::~KGLShader` (0x14001eed0) | 0x1100ec0 | C | frees CSP's variant tables (+0x40, +0x58, +0x70) |
| `KGLShader::createVertexShader / createPixelShader` (0x14001f490 / 0x14001f550) | - | D | called by CSP with the blob it built |
| `ShaderManager::ShaderManager` (0x1402278b0) | 0x1109620 | B | stores a de-obfuscated (string reversed at run time) name of a CSP compute shader `utlPrepareMesh_cs.fxo`, calls the original, captures the singleton (RVA 0x26abf30) |
| `ShaderManager::getShader` (0x140227b40) | 0x1109920 | C | name -> file resolution, CSP-first, 17,625 bytes (1.4) |
| `Shader::Shader` (0x140219de0) | 0x10fdbb0 | B | original + extension init (0x10fe350) |
| `Shader::apply` (0x14021a3b0) | 0x10fde70 | C | same variant-binding code as `GraphicsManager::setShader` |
| `Shader::reflectVars` (0x14021a550) | 0x10fde90 | B | original, then marks every reflected variable with a "known to CSP" byte (+0x34) from a CSP name table |
| `ShaderVariable::ShaderVariable` (0x140209460) | 0x10fe370 | B | before the original, a variable named `g` + upper-case letter (any name longer than 4 chars; `bones` is excluded) whose reflected type code is not one of the plain vector sizes gets type 4 `[interp]: CSP shader globals are read as float4 constants`; clears +0x34 afterwards |
| `ShaderVariable::set` (0x140209600) | 0x10fe4f0 | C | memcmp-and-copy into the cbuffer shadow copy; sets the dirty byte only on change |
| `Material` ctor x2 (0x140209970) | 0x11831d0, 0x11832a0 | B | original, then writes id = 0xFFFFFFFF at +0x2c and flag bytes +0xa9..+0xae from global switches, registers the material (0x1187cd0) |
| `Material::~Material` (0x140209d60) | 0x1183300 | C | skip on fast exit; frees variable and texture-slot vectors, drops the unused-variable sentinel entries |
| `Material::getVar` (0x14020ab30) | 0x11835e0 | C | linear name search; counts uses (+0x80) with a float sentinel -3944.2615 at +0x84 to track "never read" variables |
| `Material::initShaderVars` (0x14020ac30) | 0x1183f50 | C | full re-implementation (16,299 bytes): rebuilds the material's variable table (0x90-byte wrappers: name + value storage 0x20..0x88) and the 0x50-byte texture-slot table from the shader's reflection; this is the "new material system" data layout |
| `Material::createCBuffers` (0x14020a940) | 0x1183890 | B | loading-time pump, resets the material's cbuffer list, calls the original, then prunes buffer kinds 4, 5 and 0xD unless the shader is `ksParticle` (kind 5) / `ksPer...` |
| `Material::setShader` (0x14020b700) | 0x1183c10 | C | consults the **shader replacement** table (0xb75590: `ALLOW_TAG_NEW_CARPAINT` / `NEW_WATER` / `PBR` family) except for `ksFlagsFX` and names starting `custom_nv/`; no-op if the shader name is unchanged |
| `Material::apply` (0x14020a6e0) | 0x1186230 | B | CSP apply (0x1185ce0); on failure logs "Failed to apply material: <name>" |
| `Material::setTexture` (0x14020b740) | - | D | |
| `MaterialFilter` ctor / `apply` (0x140219d30 / 0x140219d90) | 0x1188ff0 / 0x1189070 | B / C | `apply` is re-implemented: hash of the material pointer, calls `Material::apply` only when the material changed |
| `MaterialFilterSM` ctor / `apply` (0x140229840 / 0x140229a00) | 0x1189870 / 0x1189b30 | B / C | shadow-pass filter: material cache, then picks cull mode 1, 9 or 2 from the material's double-sided flags (`DOUBLESIDED_SHADOWS_*` family), after resetting blend / depth state |
| `MaterialFilter(SM)::apply` lookups | - | D | |

### 2.3 Scene graph, meshes, models, loading

| Class / method (acs.exe VA) | CSP RVA | Class | What CSP changes |
|---|---|---|---|
| `Node` ctor x2 (0x14020d9c0) | 0x11993a0, 0x1199450 | B | original, then sets class byte (+0xb5 = 1), assigns a **node id** at +0xdc (0x1198d20) and creates / copies a CSP per-node extension record (0x1099d70 returns it: flags at +0x160, state at +0x150..+0x188, child list +0x188); bit 0x80 of the flags follows a global |
| `Node::findChildByName` (0x14020de40) | 0x1198b30 | B | on the main / loading thread first asks a CSP name index through a callback cell (RVA 0x2697798, enable byte 0x26977a8), then falls back to the original linear walk (recursive when asked) |
| `Node::addChild` / `removeChild` (0x14020dd80 / 0x14020e4a0) | 0x1199840 / 0x11996f0 | B | scene-edit lock counter (RVA 0x26a9d0c), per-node extension update, notification object at 0x26a9d18; `removeChild` honours a "pending delete" flag bit 0x1000 |
| `Node::render` (0x14020e510) | 0x630ab0 | C | 9,716 bytes: special cases for class 9 nodes (mirror / reflection helper passes with an inverted matrix), for the three camera root nodes, and per-pass flags (RVA 0x268b685 main, 0x268b687 shadow, 0x268b686 cubemap, 0x268b43c alt. shadows); read only to line 260 |
| `Node::~Node` (0x14020dc60) | 0x1198ef0 | C | 4 KB, frees extension records `[skimmed]` |
| `NodeBoundingSphere` ctor / `render` (0x140218a30 / 0x140218b80) | 0x119fb30 / 0x119fb80 | B / B | `render` writes a per-car value (car+0x288 -> +0x10) into the per-object cbuffer extension slot at +0x44 and restores 1.0 afterwards |
| `NodeEvent` ctor (0x14021e4c0) | 0x62c440 | B | `[skimmed]` |
| `Renderable` ctor x2 (0x14021c260) | 0x10f0950, 0x10f0990 | B | class byte 0xE, per-renderable ext init |
| `Mesh` ctor x2 (0x140225ab0) | 0x118a440, 0x118a4a0 | B | class byte 0xF, clears CSP flags (+0x149, +0x14c) |
| `Mesh::compile` (0x140225f00) | 0x11903c0 | B | during loading it just records the mesh in a vector (RVA 0x2704ae0); otherwise either compiles at once or, with the async flag, posts it to CSP's loading threadpool (counter RVA 0x26ada90) |
| `Mesh::render` (0x1402261a0) | 0x118f0f0 | C | CSP's draw dispatcher: depth-only / main / reflection variants, per-mesh flags (+0x149 bit 1 = custom renderer 0xea6a70, +0x8a bit 15), the per-object cbuffer refresh (0x10b3ea0 on four buffers) and the draw (0x118ddb0) |
| `Mesh::~Mesh` (0x140225dc0) | 0x118a580 | C | releases the shared vertex / index buffer refcounts (dedupe) |
| `SkinnedMesh` ctor / `~` / `compile` / `updateBoundingSphere` (0x14022bc50 / 0x14022bf30 / 0x14022c910 / 0x14022cf30) | 0x11c3b70 / 0x11c3c30 / 0x11c3eb0 / 0x11c4500 | B / C / A / C | `~` logs "Vertex data leak: " when a shared buffer is freed with users left; `updateBoundingSphere` replaced by a fixed unit sphere unless the node ext flag 0x40 (custom BS) |
| `SkinnedMesh::render` (0x14022cdc0) | 0x11c4d60 | C | visibility test (0x1094f30) then CSP's skinned draw (0x11c4af0) |
| `SkinnedMesh::initBonesBuffer / updateBonesBuffer` (0x14022cb30 / 0x14022ce90) | - | D | CSP calls them (custom skinned draw) |
| `Model` ctor / `load` (0x140217c20 / 0x140217d30) | 0x11987c0 / 0x1198820 | B / B | `load` re-formats the path as `content/showroom/<base>/<base>.kn5` from the last path component before the original `[interp]: showroom model path normalisation` |
| `WorldMatrixTraverser::traverse` (0x14021abf0) | 0x11300b0 | C | profiler zones "Scene traverser" / "Post-traverse processing"; sorts the car list (RVA 0x26ae278) by distance every third frame; origin shift into the root matrix; chooses the plain or the "shifted" traversal (0x1131340 / 0x1131540) for the main root and for each car's node tree; fires 0x2697518 at the end |
| `BoundingFrustum::setMatrix` / `~` (0x140229290 / 0x140228a80) | 0x108def0 / 0x108dea0 | B | in the main pass (not shadow) the frustum is rebuilt with the CSP "adjusted projection" matrix (RVA 0x2696a4c) |
| `CameraMeshFilter::isVisible` (0x140219af0) | 0x10951f0 | C | CSP visibility: LOD distance, pass masks (+0xe0..+0xe3), the 3-D distance test through the node bounding sphere, chunk optimisation |
| `KN5IO` ctor / `~` (0x1402145a0 / 0x140214680) | 0x10bf810 / 0x10bf840 | B / B | ctor writes one 64-bit constant into a CSP global before the original (it concerns AC's own KN5 key handling for stock content; **not examined further, rule 2**); `~` returns a 16-bit KN5 id to a lock-free free list |
| `KN5IO::load` (0x1402151a0) | 0x10c1720 | B | synthesises the `content/cars/nul/collider.kn5` tree (`collider`, WHEEL_LF/RF/LR/RR, SUSP_LF/RF/LR/RR nodes), logs "KN5: <file>", wraps the original with a "currently loading" context and the KN5 id table; 25 KB, read in outline |
| `KN5IO::loadBinaryV2` (0x140215aa0) | 0x10c3490 | B | 11.6 KB, no strings; the mesh-binary parse wrapper `[skimmed]`, belongs to `OPTIMIZE_MESHES`, `SHARED_MESHES`, `KN5_STREAMING`, `STRIP_PHYSICS_DATA` `[interp]` |
| `KN5IO::loadMaterialsBinary` (0x140216240) | 0x10c31f0 | B | sets the thread-local current-file context, calls the original, then registers the loaded materials (0x28-byte records at +0x30..+0x38) in CSP's material / texture-path map (0x10f3470) |
| `KN5IO::loadTexture` (0x1402171a0) | 0x10c5de0 | C | reads the texture name / size from the file, resolves the skin-override path (0x10c5c10 + 0x10c5890), texture cache lookup (spin-locked map), else CSP's loader (0x10f1950 -> 0x6f5860); the whole `loadTexture` family is the "texture deduplication" path |
| `KN5IO::addTextureFolder` (0x140214e90) | 0x10bfaa0 | C | de-duplicating insert into the folder list (+0x48) |
| `KN5IO::getSkinOverridenTexturePath` (0x140214f90) | 0x10c5890 | B | loading-time pump + original |
| `KN5IO::addDLCKey` (0x140214de0) | - | D | **lookup only**: CSP can call AC's own function; nothing is modified. `KN5IO::dlc_keys` data symbol also only looked up |
| `ResourceStore` ctor (0x1401fff90) | 0x10f0d10 | B | captures the singleton (RVA 0x26aba78), original, ext init |
| `ResourceStore::getTexture` (0x1402000d0) | 0x10f1930 | C | cache -> `GetFileAttributesW` -> CSP loader 0x6f5860; returns the (handle, path) pair |
| `ResourceStore::getTextureFromBuffer` (0x140200250) | 0x10f1950 | C | cache; else loading-context lookup, signal 0x2697400 (listeners may substitute the texture), then CSP creation (0x10f47c0) |
| `ResourceStore::hasTexture` (0x1402003c0) | 0x10f18f0 | C | cache query |
| `ResourceStore::~ResourceStore` (0x1401fffe0) | 0x2db1f0 | C | no-op |
| `Animation::load` (0x140207b30) | 0x58bd00 | B | remembers the last animation file name in a global (RVA 0x25e14a8) for diagnostics |
| `AnimationBlender::addTargetNode` (0x14021b060) | 0x58bd50 | A | null guard |
| `AnimationPlayer` ctor / `activateNodes` / `setCurrentPos` (0x140208570 / 0x140208b80 / 0x140209040) | 0x10e2ef0 / 0x10e3770 / 0x10e2cb0 | C / C / B | ctor re-implemented (11.9 KB): tolerant of missing target nodes ("Target animation node is missing: ", `__dummy` stand-in, `animations/steer.ksanim` default); `[skimmed]` |
| `Curve::load` / `getValue` / `getCubicSplineValue` / `getMaxReference` (0x140206a50 ...) | 0x5bb080 / 0x5bb2d0 / 0x5bb250 / 0x5bb2b0 | B / C / C / C | `load` accepts the inline form `__INLINED:` + data string besides a file name |
| `SplineStripBuilder` ctor / `addPoint` / `buildMesh` (0x14021b280 / 0x14021b6b0 / 0x14021bb20) | 0x6a14d0 / 0x6a1510 / 0x6a21b0 | C | ideal-line / strip mesh builder replaced `[skimmed]` |
| `TextNode` ctor / `render` (0x14021ad10 / 0x14021ae50) | 0x6a4f90 / 0x6a50a0 | B / B | per-node ext hook; `render` only draws in the opaque slot |
| `mat44f::createLookAt` (0x140024090) | 0x10a8bb0 | B | original then an optional matrix override from a CSP global |
| `sphere::transform` (0x140218c00) | - | D | |

### 2.4 Cameras, shadows, reflections, sky, mirrors

| Class / method (acs.exe VA) | CSP RVA | Class | What CSP changes |
|---|---|---|---|
| `Camera` ctor (0x14020ea40) | 0x1099df0 | B | original + ext init |
| `Camera::render` (0x14020f160) | 0x109ac10 | B | when the "scripted" flag +0x1be is set the render root argument is replaced with CSP's root (RVA 0x2695f88) |
| `Camera::renderCamera` / `renderCameraCustomPerspective` (0x14020f5d0 / 0x14020f720) | 0x1099fa0 / 0x109a060 | B | cancelable signal 0x2695f68 with a "continue" byte; closures keep the call args alive; the original is skipped if the listener cleared the byte |
| `Camera::clearBuffers` (0x14020ee60) | 0x109ac30 | B | cancelable signal 0x2695f70 |
| `Camera::getViewMatrix` (0x14020eff0) | 0x109acc0 | B | applies origin shift to the camera position; for the main camera fires 0x2695f78 and substitutes CSP's scripted view matrix (global at RVA 0x26b44c0 + 0x140) once |
| `Camera::getPerspectiveMatrix` (0x14020ef00) | 0x109ae80 | B | main camera (cell RVA 0x26aaf60): adaptive clip planes / FOV rules (CSP routine 0x109af10, undecompiled); the second known camera (cell RVA 0x2695f60) gets the constant 0x3d408312 (about 0.047) written into its field +0x70 when the session setting byte is not 2; afterwards the projection is copied to CSP's camera ext data (+0x180) |
| `CameraForward` ctor (0x14021f230) | 0x109e750 | B | with render scale != 1 and flag param set, constructs inside a scope guard that applies the scaled size (0x119fe70) |
| `CameraForward::onResize` (0x14021f990) | 0x109e840 | B | same scope guard, plus CSP resize bookkeeping (0x10a0b50) |
| `CameraForward::render` (0x14021fc40) | 0x109eb40 | B | origin shift on the camera position around the original; sets the "rendering the world" byte (RVA 0x268b1f7) |
| `CameraForward::renderBlurred` (0x14021fde0) | 0x109ec00 | B | signals 0x2696018 (pre) and 0x2696020 (post) |
| `CameraForward::solveBlur` (0x140220280) | 0x109e950 | B | with scale != 1 sets the viewport to the unscaled size for the blur solve, restores after |
| `CameraForward::setCubemapSize` (0x1402201b0) | 0x109eac0 | B | cancelable signal 0x2696010 |
| `CameraForward::~CameraForward` (0x14021f770) | 0x109e930 | B | skipped on fast exit |
| `CameraForwardYebis` ctor (0x140025db0) | 0x109f610 | B | original + ext init (0x10a1480) |
| `CameraForwardYebis::render` (0x14002a450) | 0x10a0bf0 | B | origin shift (when the camera uses the YEBIS flag +0x48), signal 0x2696050 after, then restores the viewport to the scaled target size |
| `CameraForwardYebis::renderApplyEffect` (0x14002a740) | 0x10a0d60 | B | stores the camera in a CSP cell (RVA 0x26ab020), signal 0x2696030, original |
| `CameraForwardYebis::applyPostProcessing` (0x140025c30) | 0x10a0ff0 | C / B | zone "Post-processing"; CSP's own pipeline (0x10a1330) when the PFX object cell is non-null, else original with a one-time "PFX is null!"; saves / restores three camera fields (+0xdd0, +0xdd4, +0xe54) and the GM +0xc; invalidates the shader cache cells afterwards |
| `CameraForwardYebis::initRenderTargets` (0x140029a90) | 0x10a09a0 | B | signal 0x2696048; target size from CSP's override (cell RVA 0x26a9a78) or the pixel-density scale |
| `CameraForwardYebis::initPostProcessing` (0x140024ca0) | 0x10a07b0 | B | wraps in a CSP scope, then optionally re-sets the YEBIS destination target (`pcRam...bf10`) |
| `CameraForwardYebis::updateYebisParameters` (0x140029e30) | 0x10a06d0 | B | the time step given to YEBIS becomes 1.0 for the first 10 frames after (re)initialisation (frame counter RVA 0x26add30 vs. the init frame at 0x26ab010) and a tiny value (0x800000) while byte 0x268b6a4 is set; the field of view is forced to 90 degrees for the 360 / fisheye camera classes (vtable cells RVA 0x268c918, 0x268c9a0) and for two sub-modes of the third camera class (0x268bca8), 100 degrees for its other mode; then original |
| `CameraForwardYebis::loadPPSet` (0x140029960) | 0x109f870 | C | 11.8 KB: filter-set loading with CSP error handling ("Both default filter and filter set in your AC video settings are missing ...", "Would you like to run validation now?" with a `steam://validate/244210` link) `[strings only]` |
| `CameraForwardYebis::readPPSetOptions` (0x140026150) | 0x10a03f0 | B | pre-processes the filter ini (0x10a01b0: key renames / HDR extras) and wraps the read |
| `CameraForwardYebis::resetYebisStates` (0x14002a650) | - | D | |
| `YebisPP` ctor (0x14002ba30) | 0x11cd750 | B | creates the wrapper's three 0x30-byte state blocks; with SBS-VR flags doubles the width; allocates a shared 0x48-byte offscreen helper with a format 0x1c (R8G8B8A8) 1x1 texture when the sizes differ |
| `YebisPP::applyPostProcessing` (0x14002bb70) | 0x11cdc50 | C | CSP routine 0x11cdd20, saves / restores fields +0x5b0, +0x5b4 |
| `YebisPP::updatePPCameraValues` (0x14002bcc0) | 0x11cda60 | B | for the main camera computes the FOV override like `updateYebisParameters` and fires 0x2695f80 |
| `YebisPP::loadPPSet` (0x14002cc80) | 0x11cdf70 | B | with the debug flag and filter name `default` swaps in the debug filter from the camera data (+0x358) |
| `YebisPP::updatePP` (0x14002c270) | 0x11cdc40 | C | sets +0x654 |
| `YebisPP::~YebisPP` (0x14002bb20) | 0x11cd9f0 | B | frees the three blocks then the original |
| `YebisPP::initPP`, `readPPSet` (0x140030540, 0x14002cd60) | - | D | |
| `KGLYebisParticleManager::setupPfxParticles` x2 (0x140024bb0) | 0x60a490, 0x60a8a0 | C | rebuilds the 0x38-byte YEBIS particle records per emitter with origin shift, a distance fade (when the debug-fade byte is set) and half-float table lookups (tables at RVA 0x21efd00..0x21efe80), then YEBIS' draw-submit slot 0x268c088 |
| `KGLYebisParticleSet::update` (0x140024950) | 0x2db1f0 | C | no-op: CSP's particle module owns this data |
| `CameraShadowMapped` ctor (0x14020bdd0) | 0x10a5ef0 | B | first instance is saved at RVA 0x26964f8; forces a field to 1 across the original; logs "Changing shadow resolution: <n>, splits=<n>" (0x10a4a10), makes sure there are >= 4 split slots (0x10a6fc0) |
| `CameraShadowMapped::createShadowMapMatrix` (0x14020c790) | 0x10a5c10 | B | cancelable signal 0x2696068 (custom shadow matrices, `[CUSTOM_SHADOW_MATRICES]`); origin shift into the light eye; fixes a degenerate (0,0) direction by an epsilon |
| `CameraShadowMapped::setShadowMapsSplits` (0x14020d5f0) | 0x10a5fa0 | B | cancelable signal 0x2696060 (`[BASIC] AUTOMATIC_SPLITS` etc.) |
| `CameraShadowMapped::shadowMapPass` (0x14020d6a0) | 0x10a6060 | B | re-entrancy guard (+0x3e0), zone "Sun shadows" around CSP's cascade render (0x10a4c20), signals 0x2696078 / 0x2696080, then a second pass over the **alternative shadows set** (map key 0x400) when present, with the alt-shadows byte (RVA 0x268b43c) set (zone "Alt. shadows set", routine 0x10a34e0). The row appears twice in the table: one hook (0x28bc56) and one extra lookup-site (0x14fa576) |
| `CameraShadowMapped::~CameraShadowMapped` (0x14020c1e0) | 0x2db1f0 | C | no-op |
| `CameraMirror::renderOpaque` / `renderTransparent` (0x14021bd80 / 0x14021bf30) | 0x1194d40 / 0x1194e30 | C / B | CSP's mirror camera object (cell RVA 0x26adac8): updates its matrices from GM+0x420, copies the view / projection back (0x1193d20); transparent pass skipped when CSP says the mirror is disabled |
| `CubeMap` ctor (0x14022b400) | 0x10b4c40 | B | original + ext init (0x10b5770) |
| `CubeMap::apply` (0x14022b480) | 0x10b51d0 | C | binds the cube SRV at PS slot 13 from the CSP cache (+0x50) |
| `CubeMap::beginFace` (0x14022b490) | 0x10b4f90 | B | per-face redirect to a CSP face target (hash-map keyed by cubemap id) + signal 0x2697218 |
| `CubeMap::endFace` (0x14022b4a0) | 0x10b50c0 | B | signals 0x2697208 (once per set) and 0x2697220 |
| `CubeMap::generateMips` (0x14022b4b0) | 0x10b5230 | B | cancelable signal 0x2697228 |
| `CubeMapRenderer::render` (0x14021edb0) | 0x10b7240 | B | zones "Reflections cubemap" and "Rendering scene"; picks a reflection centre per `[REFLECTIONS]` mode (camera / car body / centre between car and camera, smoothed), pass id 0xD, cancelable signals 0x2697238 / 0x2697248, a "faces per frame" override (0x25e7dd4 multiplier), exposure-mask pass (0x10b8570) |
| `GLRenderer` ctor / `begin` / `end` (0x1401fdfe0 / 0x1401fe800 / 0x1401fe870) | 0x10b8eb0 / 0x10b8ef0 / 0x10b8f20 | B / C / B | ctor captures the singleton (RVA 0x26ab788); `begin` applies a global colour override; `end` has cancelable signal 0x2697250 |
| `GLRenderer::fullScreenQuad` (0x1401feaa0) | 0x10b9780 | B | the AA / upscale hook (1.9): signals 0x2697258 (ksFXAA quad, zone "Post-processing anti-aliasing") and 0x2697268 (the second camera shader); both cancelable |
| `GLRenderer::vertex3f` (0x1401ffc20) | - | D | |
| `SkyBox` ctor (0x14021c790) | 0x11c5630 | B | captures singleton (RVA 0x26addf0) |
| `SkyBox::render` (0x14021d0d0) | 0x11c5670 | B | cancelable signal 0x2697a30 (a listener draws CSP's sky); otherwise writes a constant diagonal matrix (table at RVA 0x22c9f60 on the identity pattern 0x22c80f0) into the per-object cbuffer and its extra block (RVA 0x2696890 / 0x26968a0) before the original |
| `SkyBox::updateCloudsGeneration` (0x14021db00) | 0x11c5850 | B | cancelable signal 0x2697a28; progress zone "Clouds" / "Initializing" |
| `HDR::begin` (0x14022e0d0) | - | D | |
| `DynamicBuffer::commit` (0x14021c4e0) | - | D | |
| `OculusTexture::Init` (0x140212310) | 0x11c6010 | - | VR, excluded |

### 2.5 Particles, fonts, text, misc `KunosSim.lib`

| Class / method (acs.exe VA) | CSP RVA | Class | What CSP changes |
|---|---|---|---|
| `ParticleSystem` ctor (0x14025fbc0) | 0x10e0e90 | B | class byte 8 + ext init |
| `ParticleSystem::step` (0x140260df0) | 0x10e0ed0 | B | skipped when the "no particles" camera byte is set, in session type 0xB or when CSP's particle module owns the system; signal 0x2697330 after the original |
| `ParticleSystem::render` (0x1402607e0) | 0x10e0f70 | B | signals 0x2697338 before, 0x2697340 after the original; skipped when the module owns it |
| `ParticleSystem::clearParticles` (0x140260340) | - | D | |
| `StaticParticleSystem` ctor / `finalize` / `render` (0x14025e8b0 / 0x14025efb0 / 0x14025f550) | 0x1110390 / 0x11103d0 / 0x1110870 | B / B / C | `finalize` registers the system in a 0x78-byte-per-entry global list (RVA 0x26abf50) and signals 0x2697438; `render` is a distance-culled instanced draw (`IASetVertexBuffers`, `IASetIndexBuffer`) with zone "Spectators/%s"; strings `ksParticle_spectator`, `@PARTICLE_SPECTATORS`, `accShadowDynamic_spectators_*`, `data_spectator_brightness_fix.ini`, `textures/people/allowed.ini` show it is the **crowd / spectator** renderer `[interp]` |
| `StaticParticleSystem::addParticle` (0x14025ef60) | - | D | |
| `Font::blitString` (0x140200e20) | 0x1115cc0 | B | callback cell RVA 0x2697490 (returns "handled"), then original with a +offset on y for size 50 mode 2 |
| `Font::cleanup` (0x140200f30) | 0x1115ca0 | A | skipped on fast exit |
| `Font::Font` x2 (0x140200a60), `StringBlitter3D::blitString(V2)` (0x140210110, 0x140210980) | - | D | |
| `FW1FontWrapper::CFW1Factory::CreateFontWrapper` x2 (`FW1FontWrapper.lib`, 0x1404415a0) | 0x6da200, 0x6da830 | C / B | the C one builds the font wrapper from CSP's glyph render states / glyph provider / glyph atlas on the real DWrite factory (`real_DWriteCreateFactory`, shared); the second only captures the factory pointers |
| `GameTime::update` (0x14044c250) | 0x5dc7c0 | C | **trap**: logs "Should not be called anymore" and breaks; CSP replaced all callers (belongs to the main-loop brief) |
| `DebugLog` ctor / dtor (0x140217ef0 / 0x140218360) | 0x6cf630 / 0x2db1f0 | B / C | log redirection, see `attach.md` and the config brief |
| `CrashGuard::CrashGuard` (0x140218800) | 0x6c8b70 | B | see `attach.md` 1.7 and 1.8 |
| `CCollectionLoader::CreateEnumeratorFromKey` (0x140017de0) | - | D | `kgl.lib` lookup |

### 2.6 `kgl*` plain-name registrations (the D3D11 wrapper layer)

The 29 plain names below are **not** in `csp_hook_table.tsv` (it only holds `::` names). The record layout of
attach.md 1.7 is used by 24 small registration functions at dll RVA 0x22c960..0x2a7ec0; I decoded the name, the
replacement address and the slot from the three `lea` instructions of each (`dwrite_copy.dll` bytes, read only).
`[dll]`. The same walk gives the data-symbol lookups at the end.

| Name | acs.exe VA | CSP replacement RVA (size) | slot RVA | Class | Notes |
|---|---|---|---|---|---|
| `kglInit` | 0x1400184b0 | 0x6c5d00 (85) | 0x26a8638 | B | string "Running in safe mode with desktop resolution." |
| `initDX11` | 0x14001a590 | 0x107ce00 (615) | 0x26aa900 | B | "Initializing DirectX" |
| `createDeviceAndSwapChain` | 0x14001a1d0 | 0x6c3f50 (4,605) | 0x26a8620 | C | decompiled in `decomp_req1`: CSP's flip-model device + swap chain (2.7) |
| `D3D11CreateDeviceAndSwapChain` (`\D3D11\` module name) | import | 0x6c5170 | - | B | ORs `BufferUsage` with 0x400 (UAV) / 0x10 (SRV) from the hidden `UAV_OUTPUT2` / `SRV_OUTPUT3` flags, creates a DXGI factory if none, can pre-initialise an offscreen MMF device |
| `kglOpenWindow` | 0x140030c00 | 0x3f5690 (1,065) | 0x2699828 | C | window creation / placement ("Placing AC window (rect: ...", 0x3f30b0 / 0x3f3860); DPI, border, topmost |
| `kglSaveScreenCapture` | 0x1400187b0 | 0x69ae40 (171) | 0x26a6f70 | C | `[skimmed]` |
| `kglCreateSampler` | 0x140018be0 | 0x6c5aa0 (326) | 0x26a8630 | C | "d3d::device()->CreateSamplerState(&desc, &ret)" (mip bias / anisotropy) |
| `kglInitFonts` | 0x140018210 | 0x2db1f0 | 0x26a8fa0 | C | disabled (CSP creates fonts itself) |
| `kglSetFontLocale` | 0x140018540 | 0x6d9c50 (194) | 0x26a8fa8 | C | |
| `kglCreateFont` x2 | 0x1400198b0 | 0x6d9d20 (1,241), 0x6da510 (794) | 0x26a8fb0, 0x26a8fc0 | C | "KGL CREATE FONT %s with locale %s", "Failed to create a font: ", CSP glyph atlas |
| `kglCreateTextureFromFile` | 0x1400190e0 | 0x6f5860 (169) | 0x26a9308 | C | the call target of `ResourceStore::getTexture` |
| `kglCreateTextureFromBuffer` | 0x140019210 | 0x6f5910 (182) | 0x26a9310 | C | |
| `kglSetDefaultState` | 0x140018920 | 0xea4850 (21) | 0x26a9c88 | C | |
| `kglClearDepth` | 0x140018fb0 | 0x10a9980 | 0x26ab238 | B/C | |
| `kglCBufferMap` | 0x140019bb0 | 0x10b3740 (42) | 0x26ab708 | C | |
| `kglDestroyCubeMap` | 0x140018b00 | 0x10b4f60 | 0x26ab748 | B | |
| `kglDestroyVertexBuffer` | 0x14001a0a0 | 0x10bee30 (62) | 0x26ab7b8 | C | |
| `kglSetSamplerPS` | 0x140018cf0 | 0x1104af0 (38) | 0x26abf20 | C | |
| `kglSetShader` | 0x140019dc0 | 0x1104df0 | 0x26abf28 | C | |
| `kglSetBlendState` | 0x140018ff0 | 0x117bd20 (91) | 0x26ad4c8 | C | cached state compare, index range 0..0x1d |
| `kglSetCullState` | 0x140019050 | 0x117bd80 | 0x26ad4d0 | C | |
| `kglSetDepthState` | 0x140019020 | 0x117bdb0 | 0x26ad4d8 | C | |
| `kglSetPrimitiveType` | 0x140019340 | 0x117bde0 | 0x26ad4e0 | C | |
| `kglSwapBuffers` | 0x140019080 | 0x117caa0 (16) | 0x26ad510 | C | 16 bytes: calls CSP's present routine at 0x117d700, returns 0, **never calls the original** |
| `kglResizeBuffers` | 0x1400193d0 | 0x117dcb0 (1,259) | 0x26ad570 | C | "Resize buffers: ", "dx_hooks::dx_resize_swap_chain(width, height)" |
| `kglRenderTargetClear` | 0x140018d90 | 0x117e850 (78) | 0x26ad580 | C | |
| `kglDraw` / `kglDrawIndexed` | 0x14001a1b0 / 0x14001a190 | 0x117f750 / 0x117f760 | 0x26ad5b0 / 0x26ad5b8 | C | each a 5-byte `jmp` into a CSP helper (0xEA8700 / 0xEA88D0) `[interp]: draw counters + stats`, probably what feeds the Render Stats "Scene draw calls / triangles" |
| `kglResolveRenderTarget` | 0x140019650 | 0x1194e50 (557) | 0x26adaf8 | C | MSAA resolve through `accCopyResolve_msaa2/4/8_ps.fxo` |
| `kglSetRenderTargets`, `kglSetRenderTargetNative`, `kglSetTexture` | 0x140018e40, 0x140018e10, 0x140019380 | - | 0x268b948, 0x268b940, 0x268c648 | D | registered without replacement (lookups) |
| data symbols | - | - | - | D | `device` (0x26aa8a0), `immediateContext` (0x26aa8a8), `swapChain` (0x26aa8b0), `currentLayout` (0x26aa8b8), `videoSettings` (0x26aa8c0 / 0x26a9360), `blendStates` (0x26ad2a8), `cullStates` (0x26ad2e0), `depthStates` (0x26ad308), `activeRenderTargetView` (0x26ad4f8), `activeDepthStencilView` (0x26ad500), `screenRenderTarget` (0x26ad520), `screenDepthTarget` (0x26ad530), `frameInformation` (0x26adb10), `formCurrentOffset` (0x26a6ed8), `g_pPfxContext` (0x26ab008) |

CSP caches the immediate context pointer at dll RVA 0x26aa890 and the device at 0x26aa888 and uses them from more than
40 replacement functions; **there is no vtable patch of the D3D11 context** (all calls are made *by* CSP code).
`[dll]` (grep of the vtable offsets 0x38..0x228 in `decomp_hooks`).

### 2.7 Swap chain, DXGI, frame pacing, HDR (from `createDeviceAndSwapChain` 0x6c3f50 and its helpers)

`[dll]` (strings of `decomp_req1/6c3f50.c`, log, ini):

* Replaces AC's `createDeviceAndSwapChain` entirely when the "new swap chain" byte (RVA 0x2695105) is on (the opposite
  of `OLD_SWAPCHAIN`); otherwise calls the original (slot 0x26a8620).
* `CreateDXGIFactory1`, device creation with a GPU thread priority taken from `DX_DEVICE_PRIORITY`, `IDXGIFactory2`
  swap chain "Initializing DirectX with DXGI flip model"; logs "DirectX swapchain created: flip=..., VRR=...,
  tearing=..."; flags: `ALLOW_MODE_SWITCH` (2), `FRAME_LATENCY_WAITABLE_OBJECT` (0x40), `ALLOW_TEARING` (0x800)
  depending on `ALLOW_VRR`, exclusive-fullscreen and the OS; `SetMaximumFrameLatency(MAXIMUM_LATENCY_BASE)`.
* HDR: output colour space query ("HDR color space is available", "HDR is not available.", "HDR is enabled in
  Windows settings, so let ..."), `[HDR]` of `dxgi_tweaks.ini`; the pass that converts the scene and the UI to the
  HDR swap chain lives in the CSP-own code (not in a hook).
* Present path: `kglSwapBuffers` -> 0x117d700 (**not decompiled**); Render Stats report generator at 0x7a3010
  (18,512 bytes, strings quoted in 8) lists the pacing sources (V-sync via limiter / v-blank wait at frame start /
  advanced pacing / forced V-sync detection / NVIDIA profile V-sync / borderless refresh rate).
* Frame loop pacing keys of `general.ini [THREAD_TWEAKS]`: `RENDER_TIMING=4` (high-resolution timer),
  `PHYSICS_TIMING=4`, `MMCSS_PRIORITY_*`, `RENDER_HIGHEST_PRIORITY`.
* Window: `kglOpenWindow` replacement + `Placing AC window` logic; log lines of this PC's session: "Placing AC
  window (rect: ..., requested size: 1920, 1080, monitor overlaps window, rect is good to go)".
  `[log-csp]`

### 2.8 Where CSP's extra passes live (not hooks; located by profiler-zone strings)

The hooks above are the seams. The passes themselves are ordinary CSP functions; this table is **by string**, nothing
more was decompiled. `[dll]`

| Pass / feature | Entry RVA (size) | Zone / strings |
|---|---|---|
| Depth pre-pass ("depth preparation") | 0x1324800 (3,949) | "Depth preparation" |
| G-buffer: opaque / alpha-test / wet variants | 0x14edd70 (2,490) | "G-buffer (opaque)", "G-buffer (alpha test)", "Wet G-buffer ...", "Wet puddle mask", "Wet with puddles", "Wet with drops", "Wet with wipers", "Simplified", "Windscreen reflections" |
| G-buffer config reader | 0x77d200 (1,679), 0xc94a90 (456) | "G-buffer params: ", "G-buffer is not supported: "; `data_gbuffer.ini` (24 sections) |
| Extra FX manager (G-buffer, downsample, SSAO / ASSAO / HBAO+, SSGI, SSRTGI, SSLR, fog blur, volumetric lights, TAA, motion blur, depth gather) | 0xc75e30 .. 0xc83a30 (26 functions, `ExtraFX/...`), 0xc83a30 (11,836) is the main one | "ExtraFX/G-buffer", "ExtraFX/SSAO", "ExtraFX/ASSAO", "ExtraFX/HBAO+", "ExtraFX/SSLR/Raytrace|Sample|Blur|Quality map|Blending", "ExtraFX/SS lights/SSGI/Raycast|Blur", "ExtraFX/Volumetric Lights", "ExtraFX/Fog blur", "ExtraFX/TAA", "ExtraFX/Motion Blur/Prepare|Mix|Blend", "ExtraFX/Depth gather/Reduction|Readback" |
| TAA jitter, motion buffer, debug views | 0x73a890 (3,665) | "TAA jittering scale: %.0f%%", "Motion buffer", "Debug view: SSLR" |
| Volumetric light (settings) | 0x744f40 (5,742) | "Volumetric light (expensive, use carefully)" |
| PPAA selector (CMAA 2.0, FXAA 1.0, FXAA 3.11, SMAA, MLAA, custom) | 0x1228e20 (1,132), SMAA 0x12273f0 (2,299), CMAA2 0x519a30 / 0x519ed0, FXAA3 0x11cf070 | "PPAA: using ...", `accSmaa_step{1,2,3}_*`, "CMAA2", "YebisPP FXAA intermediate" |
| Alternative YEBIS pass (post-processing replacement) | 0x11ce240 (3,122), PFX object cell 0x26ab000 | "Alternative YEBIS pass: initialization / reinitialization", `accYebisAlt_ps(_pf).fxo` |
| Final stretch / proxy output / SGSR1 / bicubic / Catmull-Rom | 0x117d670 (1,546), 0x117ce50 (2,068) | `accProxy_{bc,cm,bl,bp,sgsr1,s%d}_ps.fxo`, `accCopyResolve_msaa%d_ps.fxo` |
| Upscale driver ("Upscale", FSR PPAA, FSR CMAA prep, FSR RT) | 0x122bf50 (2,217), 0xea4d80, 0x2fd910 | "Upscale", "FSR PPAA", "FSR CMAA preparation pass", "FSR RT", "DLSS NR RT" |
| Upscale region (RTV target, UAV redirect, depth / motion / stencil regions) | 0xcc8b10, 0xcc8ee0, 0xcc9440 (2,971), 0xccbda0 (`WITHOUT_MOTION_BUFFER`) | "Upscale RTV target", "Upscale UAV redirect", "Upscale region, depth|motion|stencil" |
| FSR 2.2.1 (AMD, statically linked) | 0x16e0xxx (about 0x16e0490 `FSR2_AutoComposition`, 0x2f4200 inputs) | `ffxFsr2*` exports, "FSR2_InputMotionVectors" |
| FSR 3.1 upscaler (+ frame-generation exports) | 0x16e7xxx | `ffxFsr3Upscaler*` exports; DLL `ffx_backend_dx11_x64.dll` delay-loaded |
| Intel XeSS (1.x and 2.x) | 0x2f6200, 0x2fe970, 0x2fff30, 0x306b80, 0x306bc0 | "Loading XeSS library", "Creating XeSS context", "xess_12", "EnableXeSSInputs"; DLL `libxess_dx11.dll` |
| NVIDIA DLSS (NGX 310.9) and DLSS "Neural Rendering" | 0x16da990..0x16dafb0, 0x122c800, 0x1262400 | `NVSDK_NGX_D3D11_*`, "DLSS.Hint.Render.Preset.*", "DLSS NR RT", `DLSS_NR_AFTER_ACCUMULATION` |
| Snapdragon GSR 1 / 2 | 0x2fc960, 0x2fcb40, 0x2fcdf0, 0x2fd150, 0x2fd910 | `sgsr1_ps.fxo`, `sgsr2_{activate,convert,upscale,debug}_cs.fxo`, "SGSR2 History0/1" |
| OptiScaler (external DLL as base, XeSS or DLSS) | 0x2fe970, 0x2ff9c0, 0x2ffc30, 0x2fff30, crash 0x13cad50 | "Loading OptiScaler DLL", "Migrating OptiScaler setting: ", `OptiScaler.ini` |
| FidelityFX CAS | 0x12352f0 | "FFX CAS RT", "Copying FFX CAS output" |
| Settings readers | 0x1231050 (8,968, `graphics_adjustments.ini` keys), 0x1387350 (25,641), 0xf93590 (`RENDER_SCALE`), 0x36cfa0 / 0x36ddf0 (upscaler quality, MIP bias), 0x2e4a80 (`DLSS_PRESET`), 0x2f7e30 (`FSR3_VELOCITY_FACTOR`), 0x12d0a80 / 0x71a520 (`POST_TONEMAPPING`, HDR) | |
| Shadow stack | 0x10a4a10, 0x10a4ce0 ("Alt. shadows set"), 0x10a34e0, 0x10a6fc0 | `smart_shadows.ini` |
| VAO patches (vertex ambient occlusion) | 0x11e0fa0 (4,257, `vao-patches`, `models_`), 0x11e2050 (2,470, `vao-patches-cars`), 0x1014f40 ("VAO patch file is damaged") | strings "Applying VAO patch ", "main_geometry.vao-patch", "models.vao-patch", `ext_vaoOnly` / `ext_vaoDisable` / `ext_vaoNormal` Python names |
| Virtual mirror / HQ mirror helpers | 0x117d670 neighbours, `accVirtualMirror_ps.fxo` | |
| Render Stats app report | 0x7a3010 (18,512) | "# CSP performance report" |

### 2.9 Signals (listener vectors) found in this area

Cells are `dwrite.dll` RVAs of the pointer to the vector object (image base 0x180000000). A listener entry is 0x28
bytes with the callback at +8. "cancel" = the argument block carries a byte that stops the original. A regex over
all 960 hook decompiles finds 116 distinct cells fired by 149 functions (lower bound; the pattern misses some code
shapes); this area has 35 cells in 30 functions:

| Cell | Fired by | Position | Cancel |
|---|---|---|---|
| 0x2695f68 | `Camera::renderCamera`, `renderCameraCustomPerspective` | before | yes |
| 0x2695f70 | `Camera::clearBuffers` | before | yes |
| 0x2695f78 | `Camera::getViewMatrix` | before (main camera) | no (matrix override cell instead) |
| 0x2695f80 | `YebisPP::updatePPCameraValues` | before | no |
| 0x2696010 | `CameraForward::setCubemapSize` | before | yes |
| 0x2696018 / 0x2696020 | `CameraForward::renderBlurred` | before / after | no |
| 0x2696030 | `CameraForwardYebis::renderApplyEffect` | before | no |
| 0x2696048 | `CameraForwardYebis::initRenderTargets` | before | no |
| 0x2696050 | `CameraForwardYebis::render` | after | no |
| 0x2696060 | `CameraShadowMapped::setShadowMapsSplits` | before | yes |
| 0x2696068 | `CameraShadowMapped::createShadowMapMatrix` | before | yes |
| 0x2696078 / 0x2696080 | `CameraShadowMapped::shadowMapPass` | before / after the sun cascades | no |
| 0x2697208 / 0x2697220 | `CubeMap::endFace` | before / after | no |
| 0x2697218 | `CubeMap::beginFace` | before | no |
| 0x2697228 | `CubeMap::generateMips` | before | yes |
| 0x2697238 / 0x2697248 | `CubeMapRenderer::render` | before the face loop / before the call | yes (2nd) |
| 0x2697250 | `GLRenderer::end` | before | yes |
| 0x2697258 / 0x2697268 | `GLRenderer::fullScreenQuad` | before the AC quad (ksFXAA / second shader) | yes (both) |
| 0x2697330 | `ParticleSystem::step` | after | no |
| 0x2697338 / 0x2697340 | `ParticleSystem::render` | before / after | no |
| 0x2697400 | `ResourceStore::getTextureFromBuffer` | before | no (listener may fill the output) |
| 0x2697438 | `StaticParticleSystem::finalize` | before | no |
| 0x2697518 | `WorldMatrixTraverser::traverse` | after | no |
| 0x2697550 | `GraphicsManager::compile` | before | yes |
| 0x2697558 | `GraphicsManager::endMainRenderPass` | after | no |
| 0x2697560 | `GraphicsManager::setViewport` | before | no |
| 0x2697570 | `GraphicsManager::updateLightingSetttings` and `beginScene` | after | no |
| 0x2697a28 / 0x2697a30 | `SkyBox::updateCloudsGeneration` / `SkyBox::render` | before | yes |
| single callbacks | `Font::blitString` (0x2697490), `Node::findChildByName` (0x2697798), `KGLRenderTarget` (0x26914d8), `RenderTarget` (0x26977b8) | before | returns "handled" / modifies args |

### 2.10 CSP-own type names (`ac_ext::*`, `dx::*`, `efx::*`, `pp::*` ...; group `misc_core`)

118 rows without an `acs.exe` match are registered through the same record walker as the hooks, with **kind lookup**
(no detour; slot only). One row, `ac_ext::navigator_impl::cbuffer_map_layers`, looks like a hook in the table but the
"replacement" (0x225f820) is a data address (bad instruction data), so it is a lookup `[dll]`. They are type names
of the C++ structs that CSP uploads as constant buffers (the debug label "CBuffer bind: slot ..." and the struct
size checks use them) `[interp]`:

* `ac_ext::data_ac_ext_*` (extra_fx: `cbVLData`, `cbSLData`, `cbuffer_clip_plane`, `cbuffer_sslr` x2; fake shadows;
  grass; gui; kiosk; nice screenshots `shader_accumulation_data`; rain_fx; reflections `cbuffer_ibl`; windscreen
  `cbuffer_fx` / `cbuffer_rain`), `ac_ext::ext_<effect>_{simulate,draw}_cbuffer` for 14 particle effects (cardrops,
  fireworks, flames, marbles, miragedisruption, oilspill, pieces, skidwet, smoke, sparks, traces, windscreen, ...: 29
  rows with the `track_emitter` structs), rain (`rain_drops_impl`, `rain_maps_*`, `rain_puddles_render`,
  `rain_screen_drops_impl`, `rain_windscreen_impl`), skidmarks (`cbuffer_fade|spawn|update|render`), sky
  (`sky_fx_data_v1|v2`, `moon_fx_data`, `cloud_fx_data`, `sprite_fx_ext_data`), upscalers (`pp_upscaler_fsr1::
  cbuffer_easu|rcas`, `pp_upscaler_fsr_old::input_values`, `pp_ffxcas::input_consts`), AA (`gradj_data_fxaa3`,
  `gradj_data_mlaa`, `base_gradj_ppaa_data`), `bending_shader::cbuffer_static|dynamic`,
  `interior_cubemap_material_filter`, `interior_masking_impl::...`, `refracting_headlight_data::cbuffer_bulbs`,
  `hs_car_holder::dynamic_data`, `animated_wiper_impl::extra_rubbers_gpu`, `cbuffer_tyresfx`, `cbuffer_material`.
* `dx::*` (4): `dx_perlinworley::pw_settings`, `fake_shadows_common::{ps,vs}_data`, `vr_mirror_helper::...`.
* `efx::{normals, motion, depth, stencil}` and `pp::{hdr, depth, colorGrading3D}` (each registered twice: sites
  0xcc4341.. and 0x137d0e4.. / 0x1316b47..): names of the **G-buffer / post-processing channel kinds** `[interp]`;
  `final::clean`, `mirror::raw`, `dynamic::mirror`, `special::{driver, livery, theme}` (render-target / texture
  kinds for the scripted-texture and showroom features `[interp]`), `splashscreen::*` (5).
* `AC::DataCameraExtended`, `AC::DepthMode` (CSP's mirror of AC's enum), `hooks::car_flex_data::impl::flex_cbuffer`
  (car flex shader data, physics brief).

## 3. Config keys and Lua API

### 3.1 Files that drive this area (`extension/config`, defaults)

| File (sections / keys) | What it controls |
|---|---|
| `dxgi_tweaks.ini` (7 sections, 37 live + 9 commented keys) | `[COMMON]` `SELECT_ADAPTER=0`, `BACKGROUND_PAUSE=0`, `EFFICIENCY_MODE=1`; `[COMPATIBILITY]` `OLD_WINDOW=0`, `OLD_SWAPCHAIN=0`, `EXCLUSIVE_FULLSCREEN=0`, `ALLOW_STRETCHING=0`, `OLD_STRETCH=0`; `[BASIC]` `ADVANCED_PACING=0`, `MAXIMUM_LATENCY_ADVANCED=2`, `MAXIMUM_LATENCY_BASE=0`, `VSYNC_MODE=0`, `VSYNC_ADAPTIVE=0`, `ALLOW_VRR=2`, `FORCED_VSYNC_AWARE=0`, `SAVE_WINDOW=1`; `[WINDOW_TWEAKS]` `CONFINE_CURSOR`, `SET_TOPMOST`, `FORCE_FOCUSED`, `BORDERLESS_WINDOWED`; `[HDR]` `ENABLED=0`, `VERIFY=1`, `UI_GAMMA=0.45`, `UI_BRIGHTNESS=1.7`; `[HDR_YEBIS]` `USE_FILTER_VALUES=1`, `FORCE_LINEAR_FUNCTION=1`, `BRIGHTNESS_ADJUSTMENT=0.2`, `POST_TONEMAPPING=1`, `POST_GAMMA=0.45`, `POST_BRIGHTNESS=1.2` |
| `graphics_adjustments.ini` (22 sections, 140 live + 15 commented keys) | `[RENDER_SCALE]` `SCALE=1`, `SCREENSHOTS_SCALE=1`, `FOLLOW_DESKTOP_SCALE=0`, `QUALITY=5` (SGSR1), `SHARPEN=0.5`; `[FSR]` `ACTIVE=0`, `OLD_IMPLEMENTATION=0` (method id: 0 FSR 1.0.2, 1 old FSR port, 2 NIS, 3 DLSS, 4 basic, 5 FSR2, 6 OptiScaler, 7 FSR3, 8 SGSR1, 9 SGSR2, 10 XeSS, 11 XeSS 3, 12 FSR3/4) with 13 per-method `QUALITY_*` and 11 `SHARPEN_*` keys, `ADJUST_MIP_BIAS`, `DLSS_PRESET`, `DLSS_BIAS_MASK`, `DLSS_DLL`, `XESS_REACTIVE_MASK`, `OPTISCALER_BASE`, `FSR3_VELOCITY_FACTOR`, `DISABLE_FOR_SCREENSHOTS`; `[NEURAL_RENDERING]` `ACTIVE=0`; `[ANTIALIASING]` `MODE=CMAA` (DISABLED, DEFAULT, YEBIS, CMAA, FXAA, FXAA3, SMAA), `QUALITY=HIGH`, `FXAA3_*`, `FFXCAS_PASS=0`; `[LODS]` (distance multipliers, fallback LODs, `LIMIT_VISIBLE_CARS=1`); `[VISIBLE_CARS]` (per pass LOD A..D budgets: `MAIN=2,4,10,200`, `GBUFFER=2,4,8,12`, `SHADOWS=2,4,6,24`, `MIRROR=1,3,7,12`); `[ORIGIN_SHIFT]` `ENABLED=1`; `[ADAPTIVE_CLIP_PLANES]` `ENABLED=1`, `NEAR_PLANE=0.1, 10`, `FAR_PLANE=20000, 30000`, `FOV_RANGE=0.5, 12`, `FOV_EXP=2`, `NEAR_ADAPTIVE=1`; `[VR_CLIP_PLANES]`; `[SHADER_REPLACEMENTS]` `ALLOW_TAG_NEW_CARPAINT=1`, `ALLOW_TAG_NEW_WATER=1`, `ALLOW_TAG_PBR=1`; `[HUMAN_SHADER]`; `[ANTIALIASING_MSAA]` `CUSTOM_RESOLVE=1`, `CUSTOM_KERNEL=0`, `CUSTOM_RESOLVE_MIRROR=1`; `[COLOR_BUFFER2]` (accessible colour buffer for refraction); `[TWEAKS]` `TILT_MIP_BIAS=-2.5`, `REORDER_DRIVER_RENDERING2=1`, hidden `BLOCK_COLOR_CLEAR2`, `UAV_INPUT2`, `UAV_OUTPUT2`, `SRV_OUTPUT3`; `[FIXES]` `FIX_GROOVES_RENDERING_ORDER=1`, `DOUBLESIDED_SHADOWS_WHEELS=1`; `[SCRIPTABLE_FILTER]` (Lua post filter); `[MOTION_BLUR]` `INTERIOR_MULT=1.4`, `EXTERIOR_MULT=1.2` |
| `smart_shadows.ini` (8 sections, 33 keys) | `AUTOMATIC_SPLITS=1`, `AUTOMATIC_SPLITS_DISTANCE=200`, `AUTOMATIC_SPLITS_LAMBDA=0.9`, `INTERIOR_SPLIT=1.3, 40, 200`, `EXTERIOR_SPLIT=10, 50, 250`, `FIRST_SPLIT_ADAPTIVE2=1`, `BIASES=0.04, 0.25, 1, 4`; `[SMOOTH_CASCADES]`, `[CUSTOM_SHADOW_MATRICES]` (`LOOKLESS=2`, `FOURTH_CASCADE=1`, `FOURTH_CASCADE_DISTANCE=1500`), `[LAZIER_UPDATE]`, `[DOUBLE_FACE_BIASED]`: these are the listeners of the 0x2696060 / 0x2696068 signals and of `setShadowMapTexture`'s 4th cascade |
| `colorful_shadowing.ini` | 3 sections: shadow tint / ambient (reads the shadow cbuffer extension) |
| `custom_rendering_modes.ini` | `[MODE_360]` and `[MODE_FISHEYE]` (`FRAME_RESOLUTION=1024`, `FRAMES_MARGIN=0.1`, `MODE`, `SCALE_LINEAR`, `SCALE_ANGLE`): five or six subframes through the camera classes whose FOV `updateYebisParameters` clamps to 90 degrees |
| `extra_fx.ini` (13 sections, 83 keys) | `[BASIC] ENABLED=0` (off in the shipped default; "only works without VR, with post-processing enabled"), `[TAA] ENABLED=1`, `[SSLR] ENABLED=1`, `TRACING_QUALITY3=3`, `[ASSAO]`, `[HBAO]`, ...; the pass entries are in 2.8 |
| `data_gbuffer.ini` (24 `[FIX_...]` sections, no live keys) | per-car / per-track `SKIP_GBUFFER`, `MASK_GBUFFER`, `DELAYED_RENDER`, `GLASS_FILTER`, `ALPHA_TEST_GBUFFER(_THRESHOLD)`, `AUTOFIX`, with `material:`, `shader:`, `texture:` masks (a mesh / material filter language) |
| `general.ini` `[OPTIMIZATIONS]`, `[OPTIMIZATIONS_CPU]`, `[OPTIMIZATIONS_GPU]`, `[OPTIMIZATIONS_MEMORY]`, `[NEW_KN5_LOADER]`, `[CUSTOM_FONTS_RENDERING]`, `[THREAD_TWEAKS]`, `[DEV]` | `MULTITHREADED_LOADING=1`, `WIC_TEXTURES_LOADING=1`, `NEW_VINTAGE_LOADING=0`, `USE_CHUNKS=2`, `OPTIMIZE_MESHES=1`, `DEDUPLICATE_MESHES=1`, `DEDUPLICATE_TEXTURES=1`, `UPGRADE_TEXTURES=0`, `SHARED_MATERIALS=2`, `SHARED_MESHES=1`, `COMPRESSED_MESHES=1`, `SKIP_OVERLY_DETAILED=1`, `STRIP_PHYSICS_DATA=1`, `KN5_STREAMING=1`, `LAZY_TEXTURES_LOADING=0`, `ASYNC_D3DX_TEXTURE_CREATION=1` (hidden), `D3D_DEBUG=0`, `GRACEFUL_START=3`, `DX_DEVICE_PRIORITY=7`, `CONSIDER_OCCLUDED_FLAG=1`, `CLEAR_SHADERS_CACHE2=0`, `NANORT_SCENE_RAYCAST=2`, glyph-atlas keys (`GLYPH_SHEET_SIZE=1024, 1024`, `MAX_GLYPH_SHEET_COUNT=128`, ...) |
| `particles_fx.ini` (116 keys), `reflections_fx.ini` (17), `lighting_fx.ini` (64, `TRACK_AMBIENT_GROUND_MULT_VAO=0.7`), `fake_shadows_fx.ini` (1), `rain_fx.ini`, `windscreen_fx.ini` | the listeners that sit behind the particle / cubemap / lighting signals; **not covered here** |
| `yebisest.ini` | one line "This config is no longer doing anything" |

Per-car / per-track `[ACTIVE_ON_...]` style keys for these modules exist in the `cars/loaded` and `tracks/loaded`
trees (content brief).

### 3.2 Lua API that rides on this area (counts from `lua_sdk_functions_union.txt`, 2,728 unique names)

Cross-reference: `lua.md` 3.5 and 3.7. Counts of this file: `SceneReference` 110 methods; `render.*` 39; `ExtraCanvas`
21; `GeometryShot` 35; `ColorCorrection` 14; `ac.get/set*Pp*` / `*PostProcessing*` 48; `ac.get/set*(Exposure|
Tonemap|Cubemap|Shadow|Mirror)*` 53; `ac.get/set*(Fog|Sky|Cloud|Sun)*` 105; lights 19.

What the hooked classes expose (verified by name only, the Lua glue is in `lua.md`):

* **Scene graph** (`SceneReference`, `ac.findNodes`, `ac.findMeshes`, `ac.emptySceneReference`): find / filter /
  create / clone / parent / transform nodes and meshes, KN5 loading (`loadKN5`, `loadKN5LOD`, `...Async`),
  `getVertices` / `getIndices` / `alterVertices` (needs the `Mesh::compile` buffers), material access
  (`getMaterialPropertyValue`, `setMaterialProperty`, `setMaterialTexture`, `getTextureSlot*`, `shaderName`,
  `ensureUniqueMaterials`, `applyShaderReplacements`, `projectTexture`, `projectShader`, `setBlendMode`, `setCullMode`,
  `setDepthMode`), `setShadows`, `excludeFromCubemap`, `excludeFromSecondary`, fake shadows, boolean mesh ops,
  `raycast`. These need exactly the per-node extension record (id, flags, children, bounding data) and the new material
  variable / texture-slot tables of 2.2 / 2.3.
* **Render callbacks**: `render.on`, `render.onSceneReady`, `render.fullscreenPass`, `render.shaderedQuad`,
  `render.mesh`, `render.backupRenderTarget` / `restoreRenderTarget`, `render.bindTexture`, GL-style
  `glBegin/glVertex/glEnd`, debug shapes. Shaders are compiled at run time into `cache/lua_shaders`.
* **Post-processing / camera values**: `ac.getPp*` (48: tone-map function and exposure, glare, DOF, god rays, colour
  temperature ...), `ac.getAutoExposure`, `ac.getCameraForward`, `ac.getCameraFOV`, `ac.setPpColorTemperatureK` ...
  Source of the values: `YebisPP::updatePPCameraValues` (signal 0x2695f80), `CameraForwardYebis::updateYebisParameters`
  and the PP filter set read by `readPPSetOptions`.
* **Cubemap / lights**: `ac.getCubemapBrightnessEstimation*`, `ac.adjustCubemapReprojectionBrightness`,
  `_ac_GeometryShot:overrideReflectionCubemap`, `setAlternativeShadowsSet` (the "Alt. shadows set" of
  `shadowMapPass`), `setClippingPlanes`, `setExposure`, `setFakeCarShadows`.
* **Python bridge** `ac.ext_*` names around VAO (`ext_vaoOnly`, `ext_vaoDisable`, `ext_vaoNormal`,
  `ext_setVaoActive`, `ext_isVaoPatchLoaded`) belong to the 213-function list of attach.md 1.8.

## 4. Files it reads

* **Shaders**: embedded zip #1 of `dwrite.dll` (6,683 entries; raw 123,161,141 bytes; sizes 19 B .. 650,279 B); the
  loader also accepts `extension/shaders/main_fx/<name>_{vs,ps}.fxo`, `extension/shaders/custom_nv/...` and
  `extension/shaders/meta/<name>_meta.ini` on disk (a developer path; the folder does not exist on this PC). AC's own
  `system/shaders/win/<name>_{vs,ps}.fxo` + `_meta.ini` (72 pairs) stay the fallback. Run-time compiled Lua shaders:
  `cache/lua_shaders` (`D3DCOMPILER_47` is delay-loaded; `SHADERS_DIR` dev key).
* **Textures / models**: through AC's KN5 files; skin overrides; `extension/textures/{cars,color_grading,...}`;
  `content/cars/nul/collider.kn5` (virtual; not on disk), showroom `content/showroom/<name>/<name>.kn5`.
  Caches: `cache/meshes_metadata`, `cache/trees_alignment`, `cache/clouds.bin` (44,564,640 bytes),
  KN5 streaming (`kn5.bin`, `kn5_%s.bin`; log "[KN5Stream] Initialization") when `KN5_STREAMING=1` for configured tracks.
* **VAO**: `extension/vao-patches/<track>.vao-patch` (10 files + `storage.data`, up to 9.6 MB, e.g. Nordschleife) and
  `extension/vao-patches-cars/<car>.vao-patch` (8 files), `main_geometry.vao-patch`, `models.vao-patch`; "KN5 file
  is not found in archive", "KN5 file is damaged" are the integrity messages.
* **Config**: sections of 3.1, plus AC's `cfg/video.ini`, `system/cfg/graphics.ini`, `ppfilters/*.ini`
  (`readPPSetOptions`), `data_gbuffer.ini`, `data_spectator_brightness_fix.ini`, `textures/people/allowed.ini`.
* **DLLs loaded on demand** for upscalers (not shipped here): `libxess_dx11.dll`, `ffx_backend_dx11_x64.dll`,
  `nvngx_dlss.dll` / `nvngx_dlssnr.dll` (`DLSS_DLL`, `DLSS_NR_DLL`), OptiScaler under `cache\binaries\optiscaler`.

## 5. How it is switched on

* On by default with CSP (`general.ini [BASIC] ENABLED=1`). These hooks are **always installed**; "off" means the
  replacement runs the original path (class B) or does equivalent work with default values.
* Master gates inside the area: `[BASIC] ENABLED` of `graphics_adjustments.ini` ("disabling it might break most of CSP"),
  `dxgi_tweaks.ini` module gate (this PC: "Extension `DXGI Tweaks` disabled: not available with MSAA without
  post-processing", log line 5), `COMPATIBILITY/OLD_SWAPCHAIN` and the byte at RVA 0x2695105 choose between CSP's
  flip-model swap chain and AC's, `extra_fx.ini [BASIC] ENABLED=0` (Extra FX, G-buffer, TAA, SSLR, volumetric light,
  motion buffer: all inactive by default; DLSS, FSR2, FSR3, SGSR 2, XeSS, OptiScaler **require** Extra FX per the shipped
  hint), `custom_rendering_modes.ini` ("Extension disabled: custom mode is not selected").
* **This PC** (`video.ini`, log): post-processing off (`[POST_PROCESS] ENABLED=0`, `FXAA=1` only as the CSP route),
  `AASAMPLES=4` (MSAA), `CUBEMAP SIZE=0`, `SHADOW_MAP_SIZE=-1` (the values Content Manager writes for CSP), Extra FX
  default off, PPAA "using CMAA" (log line `PPAA: using CMAA`), "Resolutions do not match, proxied output"
  (window 2752x1600 vs. 1920x1080 scene; a stretch is used), shadow resolution log "Changing shadow resolution: 0,
  splits=4" (4 cascades, so the fourth cascade is live), AMD Radeon Pro 5500M (D3D11 feature level 11_0).
  `[log-csp]`
* Per car / per track: `data_gbuffer.ini` fixes, `ext_config.ini` shader replacement tags (`ALLOW_TAG_*`), VAO
  patches by file name, `[VISIBLE_CARS]` budgets are global.

## 6. Size

| Item | Number |
|---|---|
| Rows (hook / lookup) in the five libraries | 228 (183 / 45); KunosSim.lib 146 / 37, kgl.lib 10 / 3, ksCameraYebis.lib 19 / 3, ksParticleSystem.lib 6 / 2, FW1FontWrapper.lib 2 / 0 |
| Distinct replacement functions | 178 (427,884 bytes of pseudo-C); biggest: `KGLShader::loadShaderBinary` 42,077 B pseudo-C (7,731 B machine code), `KN5IO::load` 25,433, `ShaderManager::getShader` 17,625, `Material::initShaderVars` 16,299, `AnimationPlayer` ctor 11,936, `CameraForwardYebis::loadPPSet` 11,840, `CubeMapRenderer::render` 11,569, `KN5IO::loadBinaryV2` 11,661, `GraphicsManager::setShadowMapTexture` 10,022, `Node::render` 9,716 |
| Plain `kgl*` names | 29 (24 with a replacement), + 15 data-symbol lookups |
| Signals | 116 cells in the 960 hook decompiles (lower bound); 35 of them in this area, fired by 30 functions (2.9) |
| CSP-own type names | 118 rows (89 `ac_ext::*`) |
| Shader archive | 6,683 DXBC files, 30,879,898 B deflated, 123,161,141 B raw, hashed names |
| Cbuffer enlargement | 4 AC buffers (0x40 -> 0x50, 0xE0 -> 0x140, 0xA0 -> 0x210, 0xD0 -> 0xF0) + a 0x90 block |
| Config | `dxgi_tweaks.ini` 37 keys, `graphics_adjustments.ini` 140, `smart_shadows.ini` 33, `extra_fx.ini` 83, `custom_rendering_modes.ini` 16, `data_gbuffer.ini` 24 sections, `particles_fx.ini` 116, `lighting_fx.ini` 64, `reflections_fx.ini` 17, `general.ini` 264 keys in total |
| VAO | 10 track patches + 8 car patches on this PC (about 30 MB) |
| Read in full vs. skimmed | see 8 |

## 7. Port difficulty for rustyAC

**XL** as a whole (it is a second render pipeline), **M** for the seams alone, **S** for the plain hook layer.
`[interp]`

* **Reusable** (read `docs/atlas/renderer.md`, `docs/port/renderer_core.md`, `docs/port/postfx.md`, `docs/map/yebis.md`):
  rustyAC already has Rust twins of every hooked class (`crates/rustyac-render`: `kgl.rs`, `shader.rs`, `material.rs`,
  `scene.rs`, `camera.rs`, `forward.rs`, `cubemap.rs`, `sky.rs`, `post.rs`, `fw1.rs`, `particles.rs`, `texture.rs`,
  `gpulog.rs`) with a draw-for-draw oracle (the command log compares every `UpdateSubresource` byte). The per-class
  seams here are a handful of function pointers plus per-node / per-material / per-shader extension structs; the
  **signals** map to ordinary Rust callbacks (`Vec<Box<dyn FnMut(&mut Ctx)>>`, reverse order, cancel flag).
* **Easy, proven by the existing oracle**: no-op destructors / fast exit, `getLDRScale = 1`, the magenta clear
  colour, setShadowMapBias scaling, enlarged cbuffers (the log shows the size and the bytes), origin shift (compare the
  world matrices), KN5 `nul/collider.kn5` synthetic node tree, texture cache. Each can be compared on a recorded frame
  with `render_oracle` + `gpulog` once the game side can run with CSP's DLL loaded (open question 8 of section 8).
* **Medium**: the pass-id shader variant table, `Material` variable / texture-slot re-implementation (the 0x90 / 0x50
  records), `Node` extension records and `SceneReference`-style Lua access, `Mesh::compile` deferral, shadow cascade
  signals (smart shadows) and the 4th cascade, render scale + proxied output (render-target creation callbacks),
  reflection-cubemap centre modes.
* **Hard / large**: Extra FX (G-buffer, SSAO, SSGI, SSLR, volumetric lights, TAA, motion buffer, depth gather;
  26 functions + 6,683 shaders), the PFX replacement for YEBIS, CMAA 2 / SMAA / FXAA 3.11, six upscalers (FSR 1/2/3,
  DLSS, XeSS, SGSR 1/2 + OptiScaler: each a third-party library; for rustyAC only FSR / SGSR / CAS are sensible,
  and rule 3 forbids copying CSP's shaders: new HLSL / WGSL would be written from the public algorithms), the flip
  model swap chain and the pacing logic (Windows-only; the browser build (`rustyac-web`) has none of it).
* **What could be proven and how**: (1) by name: every hook of 2.1-2.6 against the Rust function list (the `acs_va`
  column); (2) cbuffer sizes: replay a session with CSP and log `CBuffer::init` arguments vs. the Rust port;
  (3) signal order per frame: the CSP `Render Stats` report ("Nested tasks are indented and sorted by cost") lists
  the zone tree; the zone names of 8 are the same names the port can emit; (4) per-pass pictures: the log of
  `gpulog` for a frame with Extra FX on vs. off.
* Risks: ordering / re-entrancy (many hooks test CSP globals that other CSP modules set), the amount of
  undecompiled glue (about 30 functions of 2.8), and the legal rule on shaders (`6,683` plain deflate entries, nothing
  encrypted found; not copied).

## 8. Open questions and not mapped

1. **No decompile of the glue**: 0x10a1330 (CSP post-processing replacement), 0x11cdd20, 0x1228e20 PPAA,
   0x117d700 (present), 0x1324800, 0x14edd70, the whole 0xc75e30..0xc83a30 Extra FX block, `0x1099d70` (node
   extension getter), `0x1100750` (variant selector), `0x109af10` (adaptive clip planes) were **not** read;
   everything about them above is from string and call-edge evidence.
2. **Which listener is subscribed to which signal** is not known (the `connect` calls are in non-hook functions).
   2.9 lists producers only.
3. The role of the 3 mirrored cbuffer copies (RVA 0x2696a58, 0x2696be0, 0x2696ef0) and of the pass-id values 1..4 is
   inferred. The exact AC struct behind cbuffer type 0..3 is taken from `docs/port/renderer_core.md`, not proven here.
4. `Node::render` (9.7 KB), `Node::~Node`, `StaticParticleSystem::render` (tail), `KN5IO::load` / `loadBinaryV2`,
   `AnimationPlayer` ctor, `SplineStripBuilder::*`, `CameraForwardYebis::loadPPSet`, `ShaderManager::getShader` body
   and the tail of `WorldMatrixTraverser::traverse` were only skimmed (structure and strings).
5. Frame pacing: the pacing code (waitable object, v-blank wait, advanced pacing) is not in a hook; it is reached from
   `kglSwapBuffers` -> 0x117d700 and the window loop; not decompiled.
6. HDR output and UI composition to the HDR swap chain (0x71a520, 56,802 bytes; 0x12d0a80) not read.
7. The 64-bit constant written by `KN5IO::KN5IO` concerns AC's own content protection: **not examined** (rule 2);
   `KN5IO::addDLCKey` is a lookup, CSP does not alter AC's key tables here. Mod-maker encrypted content is not touched
   by any hook of this area (the encrypted car data path is the `FolderEncrypter` hooks of attach.md 1.9).
8. Running the game's renderer and CSP's DLL together in the oracle process (attach.md 8.6) was not tried.
9. `ac_ext::*` struct layouts are only names; the field offsets live in the uploader functions.
10. VR classes (StereoCameraVive etc.) excluded as assigned; `OculusTexture::Init` listed only for completeness.

Read in full: all 22 `GraphicsManager` hooks, `CBuffer` x6, `KGLCBuffer`, `KGLVertexBuffer`, `KGLIndexBuffer`,
`KGLRenderTarget`, `KGLTexture` x2 + `initSize`, `RenderTarget`, `Camera` x6, `CameraForward` x7,
`CameraForwardYebis` x8 (not `loadPPSet`), `YebisPP` x6, `setupPfxParticles` x2, `CameraShadowMapped` x5, `CubeMap` x5,
`CubeMapRenderer::render`, `GLRenderer` x4, `Shader` x3, `ShaderVariable` x2, `KGLShader` ctor-side x2,
`ShaderManager` ctor, `Material` x8 (createCBuffers first 60%), `MaterialFilter(SM)` x4, `Mesh` x5, `SkinnedMesh` x6,
`Model` x2, `Node` ctor x2 / `findChildByName` / `addChild` / `removeChild`, `NodeBoundingSphere` x2, `ResourceStore`
x4 (`getTextureFromBuffer` 70%), `KN5IO` small x4 + `loadMaterialsBinary` + `loadTexture`, `SkyBox` x3, `Font` x2,
`FW1` x2, `ParticleSystem` x3, `StaticParticleSystem` ctor / `finalize`, `Curve::load`, `Animation::load`,
`GameTime::update`, `BoundingFrustum::setMatrix`, `CameraMirror` x2, `mat44f::createLookAt`, and from `decomp_req1`
`6c3f50` / `6c5170` (strings + control outline). About 55% of the 427 KB.

## Evidence

* `re/scratch/csp1/hook_groups.tsv` (group `rendering_hooks`, regenerated while I worked: 301 hook + 67 lookup), `csp_hook_table.tsv`,
  `dll_func_strings.tsv`, `dll_funcs.tsv`, `dll_strings.tsv`, `dll_zip_listing.json`, `lua_sdk_functions_union.txt`.
* `decomp_hooks/{117e8a0,117eb20,117eb40,117f2d0,117ca90,117ca00,117c7b0,117c870,117c8e0,117c940,117c970,117c9e0,
  117ca60,117ebd0,117ec70,117ef00,117f1b0,117f630,117f6b0,414fc0,10b3470,10b3500,10b4190,10b41f0,10b3e30,10b3680,
  10b36d0,10bed60,10bef10,6f5b80,10bb280,10bbac0,10bbf20,119fc80,11026a0,1102350,1100ec0,1109620,1109920,10fdbb0,
  10fde70,10fde90,10fe370,10fe4f0,11831d0,11832a0,1183300,11835e0,1183890,1183c10,1183f50,1186230,1188ff0,1189070,
  1189870,1189b30,118a440,118a4a0,118a580,118f0f0,11903c0,11c3b70,11c3c30,11c3eb0,11c4500,11c4d60,11987c0,1198820,
  11993a0,1199450,1198b30,1199840,11996f0,119fb30,119fb80,630ab0,11300b0,108def0,10951f0,10f0d10,10f18f0,10f1930,
  10f1950,10bf810,10bf840,10bfaa0,10c5890,10c5de0,10c1720,10c31f0,1099df0,109ac10,109ac30,109acc0,109ae80,1099fa0,
  109a060,109e750,109e840,109e930,109e950,109eac0,109eb40,109ec00,109f610,10a0bf0,10a0d60,10a0ff0,10a06d0,10a07b0,
  10a09a0,10a03f0,11cd750,11cd9f0,11cda60,11cdc40,11cdc50,11cdf70,60a490,60a8a0,10a5c10,10a5ef0,10a5fa0,10a6060,
  10b4c40,10b4f90,10b50c0,10b51d0,10b5230,10b7240,10b8eb0,10b8ef0,10b8f20,10b9780,11c5630,11c5670,11c5850,1115cc0,
  1115ca0,6da200,6da830,10e0e90,10e0ed0,10e0f70,1110390,11103d0,1110870,6a4f90,6a50a0,5bb080,58bd00,58bd50,5dc7c0,
  1194d40,1194e30,10a8bb0,2db1f0}.c`; `decomp_req1/{6c3f50,6c5170}.c`.
* `dwrite_copy.dll` bytes of the 24 plain-name registration functions and of 0x117caa0 / 0x117f750 / 0x117f760
  (read-only, printed to stdout).
* `extension/config/{dxgi_tweaks,graphics_adjustments,smart_shadows,colorful_shadowing,custom_rendering_modes,
  extra_fx,data_gbuffer,general,yebisest}.ini`; `extension/shaders.zip` (placeholder), `extension/vao-patches*`;
  `Documents/Assetto Corsa/logs/custom_shaders_patch.log` lines 5, 37, 79, 119, 910-913;
  `cfg/video.ini`.
* `docs/atlas/renderer.md`, `docs/port/renderer_core.md`, `docs/port/postfx.md`, `docs/map/yebis.md`;
  `attach.md` 1.7, 1.8, 2; `lua.md` 3.5 / 3.7; `acs_index/functions.tsv` for the kgl VAs.

## Decompile requests

1. 0x10a1330 : the CSP post-processing pipeline that replaces YEBIS `ApplyEffects` (what it chains: exposure, bloom, DOF, tone map, LUT; does it call the PPAA / upscaler listeners).
2. 0x11cdd20 : the same for `YebisPP::applyPostProcessing` (VR / triple).
3. 0x1228e20 : PPAA selector (who calls it; which signal cell it listens on).
4. 0x117d700 : present path (frame limiter, pacing, HDR conversion, proxied output).
5. 0x117d670 : "proxied output" / MSAA-resolve target switch.
6. 0x117dcb0 : `kglResizeBuffers` replacement (resize rules, scaled sizes).
7. 0x3f5690 : `kglOpenWindow` replacement (borderless / DPI / topmost).
8. 0x1324800 : depth preparation (which meshes, which shaders).
9. 0x14edd70 : G-buffer / wet passes (render-target formats, pass ids).
10. 0xc83a30 : Extra FX main routine (pass order list).
11. 0x73a890 : TAA jitter + motion-buffer production.
12. 0x122bf50 : upscale driver (selection of FSR2 / FSR3 / DLSS / XeSS / SGSR, input resources).
13. 0xcc9440, 0xcc8b10, 0xcc8ee0 : upscale region / RTV target / UAV redirect (how the scaled scene is bound).
14. 0x1099d70 : node-extension getter (record layout, +0x150..+0x188).
15. 0x1100750 : shader variant selector (pass id -> variant).
16. 0x109af10 : adaptive clip planes (formulas for near / far / FOV).
17. 0x10bb2b0 and 0x10bbaf0 : KGLTexture creation (formats, held-view path, DDS / WIC loader choice).
18. 0x6f5860 and 0x6f5910 : `kglCreateTextureFromFile` / `FromBuffer` replacements.
19. 0x10c3490 : `KN5IO::loadBinaryV2` (mesh dedupe, streaming cache, physics strip).
20. 0x10c1720 : tail of `KN5IO::load` (which cache lists, KN5 id table).
21. 0x630ab0 : rest of `Node::render` (lines after 260: the three camera cases and the pass flags).
22. 0x11e0fa0 and 0x11e2050 : VAO patch application (file format, which vertex attribute it overwrites).
23. 0x10a34e0 and 0x10a4c20 : the alt. shadows set and the sun cascade render.
24. 0x10a4a10 : shadow resolution change (log "Changing shadow resolution").
25. 0x1181180 : `GraphicsManager` extension init (what state it allocates).
26. 0x10b43a0 and 0x10b47d0 : CBuffer creation path of the enlarged buffers.
27. 0x1187cd0 : material registration at construction.
28. 0xb75590 : shader replacement table (`ALLOW_TAG_*` names and how a replacement is chosen).
29. 0x10fe650 : virtual-file reader that serves the shader archive.
30. 0x7a3010 : Render Stats report (pacing sources, zone tree).
31. 0x1131340 and 0x1131540 : the two world-matrix traversal variants of `WorldMatrixTraverser::traverse` (plain vs. origin-shifted).
32. 0x6c3f50 tail (after the swap-chain creation: HDR set-up, `dxgi_tweaks.ini` reads).
33. 0x10e2ef0 : `AnimationPlayer` ctor (missing-node policy).
34. 0x6a1510 and 0x6a21b0 : `SplineStripBuilder` (strip format, ideal line).
35. 0x1110870 tail : spectator draw (instance data layout).
36. 0x1113720 : a function with the string "Present" (checks if it is the pacing call).
37. 0xf93590 : `RENDER_SCALE` reader (quality enum -> filter).
38. 0x36cfa0, 0x36ddf0 : upscaler quality / MIP bias.
39. 0x11ce240 : "Alternative YEBIS pass" init.
40. 0x1387350 : the 25,641-byte reader with `FFXCAS_PASS` (settings struct layout, which module).
