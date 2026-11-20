# Native builds beyond Windows x64 (Task P1, spike)

macOS (Apple Silicon, Intel), Linux (x64, ARM) and Windows on ARM: what stands in the way, what
already gives the same bits, and the order to build it in. A spike: no game code changed, no
version. The only code is the manual workflow `.github/workflows/platforms.yml`.

## Resume here

- Done: the inventory (section 2), the workflow (section 3, not run yet), the bit-exactness
  section (4), the game files, FMOD and Python decisions (5 to 7), the plan (8).
- Next: Super runs the workflow (section 3.2). Read its logs and replace the "expected" column of
  section 3.3 with what happened. Then start the plan at step L1.

## 1. Plain-English summary

rustyAC already builds off Windows today, but only as the **replay runner**: `src/main.rs`
compiles a `--replay <file> --headless` program on any target that isn't Windows, the same one
the WASI build is. Everything a player sees is behind `cfg(windows)`: the window, Direct3D 11,
the devices, the sound DLLs, Python, the launcher's WebView2 and the shared memory.

The good news is that the browser already did most of the hard part. Its Full mode runs AC's own
renderer on **wgpu** (`kgl_wgpu.rs`), which is Vulkan on Linux and Metal on macOS natively. It
plays AC's banks on **FMOD 2.03**, and its physics is **proven bit for bit** against the desktop.
A native build is mostly plumbing around code that exists: a window and input layer (SDL3), the
wgpu surface, FMOD's native libraries, `wry` for the launcher, and the game-files chooser. The
first Linux x64 build is about **9 sessions**, and macOS Apple Silicon adds about **2**. The other
targets are small after that.

The physics gives the same bits on every target, because it runs AC's maths in plain Rust
(including the FMA3 paths, through a correctly rounded software `fma`). Where there is no
`rcpps` instruction it uses Intel's `rcpps` table. One thing is unproven on ARM: **the sign of a
NaN** made by an invalid operation (x86 makes a negative one, ARM a positive one), and AC's states
do carry NaNs. The workflow's ARM test jobs and one replay on a Mac settle it.

## 2. The Windows-only inventory

Sizes are lines of code today, and the estimate is the work to make each piece cross-platform
(one session is about one Opus session).

| Piece (crate: files) | Windows-only because | Replacement | Size |
|---|---|---|---|
| Game loop and window (game: `desktop.rs` 3,056, `window.rs` 225, `mouse_hider.rs` 161, `cli.rs` 1,361) | Win32 window, message loop and cursor; `cli`, `conditions`, `identity`, `join`, `gateway`, `ai_record`, `achievements` are gated only because they sit next to it (Documents via `USERPROFILE`, backslash paths) | **SDL3** (`sdl3` crate, zlib licence): window, keyboard, mouse. Split `desktop.rs` into a portable loop plus a small platform layer, and ungate the portable modules | 2 sessions |
| Renderer, Direct3D 11 (render: `kgl.rs` 1,078, `shader.rs` 510, `texture.rs` 378, `post.rs` 459, `fw1.rs` 1,015 DirectWrite; game: `render/mod.rs` 1,149 debug view, `font.rs`, `models.rs`, `picture.rs`) | D3D11, DXGI, DirectWrite | **The browser's Full mode path**: `kgl_wgpu.rs` (2,077, already done) plus `fw1_wgpu.rs`/`ttf.rs` text. Natively it needs a wgpu surface on the SDL window, present and resize. The D3D11 debug view is left out (Full only) | 1 session |
| Post-processing, YEBIS (render: `yebis*.rs` ~6,000) | Its draws are on D3D11 only (not in the browser either) | Port the 8+ draws to `kgl_wgpu` once, for native **and** the browser | 2–3 sessions, after the first build |
| Pads (game: `input/pad.rs` 831, `input/live.rs` 403) | XInput, raw input | **SDL3 gamepad** (Xbox layout, the same as XInput's) with rumble | 0.5 session |
| Wheels and force feedback (game: `input/dinput.rs` 377, the portable `wheel.rs` 719 stays) | DirectInput 8 | **SDL3 joystick + haptic** (constant force, as AC's FFB uses). gilrs has no wheel FFB. Linux: wheels with kernel drivers (Logitech, Fanatec via hid-fanatecff, Moza and Simagic via hid-pidff). macOS: best effort, few wheels have drivers | 1 session, then tests on Super's wheel under Linux |
| Physics thread timer (game: `timer.rs` 99, `physics_thread.rs` 827) | QPC, `timeBeginPeriod`, thread priority | `std::time::Instant` plus a sleep-then-spin wait. Timing never touches bits | 0.3 session |
| Shared memory, SimHub etc. (game: `shm.rs` 329) | `CreateFileMapping("Local\acpmf_*")` | **Keep, change**: POSIX `shm_open("/acpmf_physics")` etc. with the same layouts (Linux `/dev/shm`, the place Proton bridges like simshmbridge use). SimHub itself is Windows-only; Linux dashboards read `/dev/shm` | 0.3 session |
| Sound (audio: `fmod/raw.rs` loader; game: `audio.rs` 994, its sink gated) | `LoadLibraryExW` of AC's **FMOD 1.08.12** Windows DLLs | **FMOD Engine 2.03 native libraries** (`libfmod`/`libfmodstudio` `.so`/`.dylib`), shipped (section 6). The 1.08 to 2.03 differences are already bridged for the browser in `fmod/web.rs` (220): the same mapping over the C ABI, loaded with `libloading` (already a dependency). AC's two DSP plug-ins (`dsp/`) are left out, as in the browser | 1 session |
| Python apps (python: the whole crate, 2,220, `#![cfg(windows)]`) | Loads AC's `python33.dll` | **Leave out first** (as the browser does). Later: our own CPython 3.3 build per target (section 7), `dlopen` of the same 28 functions | 0.3 session binding + 2 sessions of CPython builds, later |
| Launcher (launcher: `shell.rs` 853, `sys.rs` 129, `webview2-com`) | WebView2 | **`wry`** (MIT/Apache): WKWebView on macOS, WebKitGTK on Linux (the player installs `libwebkit2gtk-4.1`). The ported bridge (`bridge.rs` 1,090 and the rest) is already portable | 1 session |
| Finding AC (content: `install.rs` registry part, ~50) | Registry, `C:\` defaults, backslashes | Linux Steam roots (`~/.steam/steam`, `~/.local/share/Steam`, Flatpak's `~/.var/app/com.valvesoftware.Steam/.local/share/Steam`) and the existing `libraryfolders.vdf` parser. Plus the chooser, check and zip (section 5) | 0.3 + 1.5 sessions |
| Maths DLL (math: `msvcr120` loader; game: `crt.rs` `expf`) | `MSVCR120.dll` | **Nothing to do**: without the DLL the `pure` backend is chosen, which is bit-identical | 0 |
| TLS for `--join wss://` (game: `rustls` dependency) | Only declared under `cfg(windows)` | Ungate (rustls + ring build everywhere) | 0.1 session |
| TrackIR (game: `trackir.rs` 294) | `NPClient64.dll` | **Leave out**. Linux players use opentrack | 0 |
| Steam (game: `steam.rs` 206, `achievements.rs` 916) | AC's `steam_api64.dll` | **Leave out**. Everything loads without Steam (the file says so): no DLC keys are checked, and the online GUID comes from the player's settings. A `libsteam_api` can't be shipped (Steamworks terms) | 0 |
| VR (render: `vr.rs` 822 OpenVR, `oculus.rs` 927) | AC's OpenVR / Oculus DLLs, D3D11 textures | **Leave out**. Oculus is Windows-only. SteamVR on Linux would mean OpenXR on wgpu later | 0 |
| Triple screens (render: `triple.rs` 170) | Nothing Windows-specific, but only tried on D3D11 | **Keep**, try it on wgpu after the first build | 0.3 session |
| CSP (`--csp`, ode `csp_*`) | CSP is a Windows DLL inside AC's folder | **Leave out** (Windows only) | 0 |
| Oracle logs (render: `gpulog.rs` 1,516; all `tools/*_oracle`) | They map `acs.exe` / log D3D11 calls | **Windows only, forever** (section 4) | 0 |

## 3. The workflow

### 3.1 What it does

`.github/workflows/platforms.yml`: **manual only** (`workflow_dispatch`), `contents: read`, no
secrets, GitHub's standard hosted runners (free on a public repo), Rust 1.97.0 from
`rust-toolchain.toml`, and `fail-fast: false`. Per target it:

1. installs rustup if the image has none, plus the toolchain and the target. It **checks that the
   runner's host is the target**, so a renamed runner label can't build the wrong thing;
2. builds `rustyac` (the game on Windows, the replay runner elsewhere), then **every crate on its
   own** so one failure doesn't hide the rest. It also builds `rustyac-render --features
   backend-wgpu` and `rustyac-game --features wgpu` (the Full-mode renderer, natively);
3. runs the **game-free bit proofs**: the tests of `rustyac-math`, `-ode`, `-physics`, `-net`,
   `-content`, `-gui` and `-audio`. These are the committed recorded hashes and goldens, and tests
   that need AC print NOT TESTED, which are counted;
4. writes a table to the run's page and uploads `logs/` (one log per row) as `logs-<target>`.

| Matrix row | Runner |
|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04` |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |
| `aarch64-apple-darwin` | `macos-15` |
| `x86_64-apple-darwin` | `macos-15-intel` |
| `aarch64-pc-windows-msvc` | `windows-11-arm` |
| `x86_64-pc-windows-msvc` (emulated) | `windows-11-arm`: the x64 build and its tests run under Windows' x64 emulation, which answers the Windows-on-ARM question of section 8 |

### 3.2 Steps for Super

1. Merge `web` into `master` and push. GitHub only offers a manual workflow from the default
   branch.
2. Start it: GitHub, **Actions → Platforms → Run workflow** (branch `master`), or
   `gh workflow run platforms.yml --ref master`.
3. Wait (`gh run watch`). It takes about 30–60 min a target in parallel. The macOS runners are the
   slowest.
4. Download the logs into the scratch folder: `gh run download <run id> --dir
   re/scratch/taskP1/run` (the id is in `gh run list --workflow platforms.yml`).
5. Give the next session the run id, or just say the logs are there.

A red target is expected (see the table below). The job is red when any row failed, so a mistake
is never shown as green.

### 3.3 Expected results (from the code, until the run is back)

| Row | Linux x64 / ARM, macOS ARM / Intel | Windows ARM64 | x64 emulated |
|---|---|---|---|
| `rustyac` | ok: the replay runner (`main.rs`'s non-Windows branch) | full game: ok if `ring` (rustls) finds clang on the image, else FAILED | ok if the image has the x64 MSVC libraries |
| each crate | ok **except `rustyac-render`**: its `compile_error!` (off Windows it needs `backend-wgpu`). `rustyac-web`'s native build has never been tried off Windows | ok, but ring as above | ok |
| `render-wgpu` | ok likely (wgpu needs no system libraries to build) | ok | ok |
| `game-wgpu` | unknown: only ever built on Windows. The most useful row | ok | ok |
| tests | ok; math's DLL sample is NOT TESTED. **ARM: watch `rustyac-ode` and `rustyac-physics`** (NaN sign, section 4) | ok (pure maths, rcpps table) | **watch `rustyac-ode`**: `rcpps` is executed by the emulator |

## 4. What stays bit-exact

**Same bits on every target already (from the code and earlier proofs):**

- **The physics, ODE and the network logic.** Every transcendental in `rustyac-physics` and
  `rustyac-ode` goes through `rustyac-math` (the ODE's `Real::sin/cos` included). None goes through
  the system's libm, whose bits differ between glibc, Apple and the UCRT. Without `MSVCR120.dll`
  the `pure` backend is chosen: the DLL's **FMA3** algorithms in plain Rust, with `fma` from
  `libm::fma` (correctly rounded everywhere, which is also what ARM's `fmadd` gives). Proofs: the
  WebAssembly build against the desktop (`docs/port/web.md` section 3), and the farm VM (a
  CPU with no FMA3, `RUSTYAC_MATH=pure`, quick physics 9/9 IDENTICAL, `docs/proof_farm.md`).
- **`rcpps`** (ODE's `_dLDLTRemove`): x86 executes the instruction, as the game does. Every other
  architecture uses `rcp_table.rs`, Intel's answers for all 2^32 operands. An Apple Silicon Mac
  therefore matches an Intel PC.
- **MXCSR**: off x86 the control word reads as 0 and setting it does nothing. ARM's default (no
  flush-to-zero) is what AC runs with, so denormals behave the same.

**Not proven yet:**

- **The NaN sign on ARM.** An invalid operation (0/0, ∞−∞) gives `0xFFC00000` on x86 and
  `0x7FC00000` on ARM. AC's states do hold NaNs (the AI line's side velocity, `web.md` 3.2), and
  so far they were only compared on x86 hosts (the browser too). Where a NaN only passes through,
  `replay_hash`'s NaN-blind hash still matches. Where code tests its sign or its bits, the drive
  could part. The proof is the workflow's ARM test rows, plus one replay on an Apple Silicon Mac:
  `rustyac --replay x.ryin --headless --dump-states` against the Windows dump, the WASI proof's
  recipe.
- **x64 under emulation** (Windows on ARM's Prism, macOS's Rosetta 2): `rcpps` runs through the
  emulator, which may not answer as Intel's table does. `MSVCR120`'s FMA3 or SSE2 choice follows
  the CPUID the emulator reports. The emulated workflow row shows the first. The fix, if needed,
  is small: use the table when `IsWow64Process2` says the machine is ARM64.

**What CI can prove without the game:** only what is committed. That is the physics goldens
(`crates/rustyac-physics/tests/golden`, 37 files, 4.6 MB: tyre, SCTM, replays.rs), the ODE's
recorded operations (`crates/rustyac-ode/tests/data`, 34 files, 1.2 MB), the maths' recorded
hashes, and the net and audio goldens (176 KB, 140 KB). All of that is already in the repo and
already in the workflow. **No kept recording can go in the repo.** A replay needs the car's and
track's own data files (AC's, rule 7), and `re/gold` is untracked (0 files in git). Replays off
Windows therefore run where AC's files are: a player's or Super's Mac or Linux PC, or a Linux VM
on the farm with Super's own copy of the files (the farm's `physics` area, already
`RUSTYAC_MATH=pure`).

**Windows x64 only, forever:** everything compared with `acs.exe` (every oracle maps it into its
own process), the render oracle against D3D11 call logs, the sound against FMOD 1.08's call logs,
CSP, and the launcher oracle (.NET, `AssettoCorsa.exe`). Off Windows the proof is "same bits as
the Windows port", which in turn is proven against the game.

## 5. Choosing the game files (every OS)

The player gets AC's files themselves (a Windows Steam install, Proton, or a copy). rustyAC
downloads nothing.

1. **Find:** `AC_ROOT` overrides as today. Next comes the remembered choice. Then Steam's library
   list: `install.rs` already parses `libraryfolders.vdf` and looks for
   `steamapps/common/assettocorsa` in each library (app 244210). On Linux it needs the Steam roots
   listed in section 2; Proton installs the normal folder. macOS has no AC on Steam, so it goes
   straight to the chooser. Documents: Proton's
   `steamapps/compatdata/244210/pfx/drive_c/users/steamuser/Documents/Assetto Corsa`, otherwise
   `~/Documents/Assetto Corsa`.
2. **Offer:** "Assetto Corsa found in <folder>. **Change…**". When none is found: "Assetto
   Corsa's install folder wasn't found: choose the folder with the game files". The choice is
   remembered in rustyAC's own settings file (`~/.config/rustyAC/` or `~/Library/Application
   Support/rustyAC/`, from `HOME`/`XDG_CONFIG_HOME`, no new crate). The dialog is `rfd` (MIT,
   native dialog, portals on Linux) or the launcher page's own.
3. **Check:** against a **required-files list** generated at build time by `tools/web_pack`'s
   probes (`content::files_read_by_drive` + `files_of_picture` + the sound banks, per car and per
   track). It holds paths and sizes only, never content. There are two lists: "base game" and
   "everything". Missing files are named, grouped by car or track, so a player missing one DLC can
   still drive the rest. Sizes: this install is 38 GB in all (`content` 33.5 GB: 209 cars
   21.4 GB, 26 tracks 10.3 GB, mods and CSP included). The base game alone was **not measured**:
   the list generator's first run gives it (step L6).
4. **A zip** of the game files: checked the same way, then **read in place, not unpacked**.
   Measured here (`re/scratch/taskP1/zipspeed.py`, Spa's 441 MB `spa.kn5`): deflate decompresses
   at **310 MB/s on one core**, 1.42 s against 1.19 s for reading the plain file. A drive already
   holds the whole model file in memory to parse it (`web.md` section 7), so in place costs about
   1.4 s more CPU per Spa load (Laguna Seca's 844 MB: about 2.7 s), no extra memory, and no second
   20–30 GB copy on disk. Stored (`zip -0`) and deflate are read. Other methods (LZMA, zstd) get
   "unzip it and choose the folder". This needs zip64 (the archive is over 4 GB): the `zip` crate
   (MIT) reads it, mounted through the existing `content::vfs`, the browser's virtual file layer.

## 6. FMOD off Windows, and the licence

- AC's FMOD DLLs are Windows binaries. Native builds ship **FMOD Engine 2.03** (2.03.15, the
  browser's version, which already plays AC's banks) from fmod.com, for Linux x64/arm64 and macOS
  (universal), under FMOD's EULA. Super registers rustyAC as a free non-commercial project in his
  FMOD profile. The credit "Audio engine: FMOD Studio by Firelight Technologies Pty Ltd." goes in
  the game's About text and the zip's notices.
- **GPL:** `LICENSING.md` **already has** the section-7 additional permission for linking or
  combining with the FMOD Engine. It is worded for any FMOD Engine, not only the DLLs, and Super
  is the copyright holder of every GPL file (no third-party GPL code is in the dependency list).
  When the first native zip ships, the wording to change is the paragraph above the permission:
  "On Windows it uses the FMOD DLLs from the player's own install … FMOD is not part of this
  repository or its releases". The new text says the macOS and Linux zips **carry FMOD's
  unchanged libraries**, under the FMOD EULA, not the GPL and not part of the Corresponding
  Source. The FMOD list entry above it gets the same change. Not changed now: nothing ships yet.
- **The current Windows build** asks the same question and is covered twice. It conveys no FMOD
  at all (it loads the player's own DLLs at run time), and the existing permission covers the
  combination anyway. Nothing to change.

## 7. Python apps off Windows

- **First builds: off**, as in the browser. The apps' `.py` files come with the player's AC
  files anyway.
- **Later: CPython 3.3 built by rustyAC** (PSF licence, GPL-compatible) per target as a shared
  library, with the same 28 functions bound by `dlopen`. Linux x64/arm64: 3.3's
  `config.guess`/`config.sub` replaced with current ones, and a few `CFLAGS` for a modern GCC.
  About 0.5 session. **Apple Silicon** came with 3.9.1, years after 3.3, so 3.3 there needs the
  new `config.guess` plus a configure patch for `arm64-apple-darwin`, a modern **libffi**
  (`--with-system-ffi`: 3.3's bundled libffi has no arm64 macOS trampolines, so `ctypes` breaks
  without it), and fixes for clang's errors on implicit declarations. About 1–1.5 sessions with
  real risk. The standard library may come from the player's own `system/x64/Python33.zip` (pure
  `.py`), so rustyAC would ship only the interpreter (not tried).
- What breaks either way: apps that bundle Windows `.pyd` files (third-party `_ctypes.pyd`
  copies), and apps that open AC's shared memory by its Windows name with `mmap(…, tagname=…)`,
  for example the popular `sim_info.py`.
- **Fallback, a modern CPython (3.12+):** AC's apps hit removed names. `time.clock` was removed in
  3.8 and many apps use it. The `collections.Mapping`-style aliases went in 3.10, `imp` in 3.12.
  A shim module could put those back. Less work than building 3.3, but each app is a gamble.

## 8. The plan

**Order:** Linux x64 first (CI-checkable, Proton players already have AC's files), then macOS
Apple Silicon, then Linux ARM and macOS Intel (the same code, one more target each). Windows on
ARM: ship nothing new until the emulated row is green.

**Parts in order** (each one committed and proven before the next):

| Step | Part | Sessions |
|---|---|---|
| L1 | Ungate the portable modules (`cli`, `conditions`, `identity`, `join`, `gateway`, TLS), Documents and Steam paths per OS | 0.5 |
| L2 | SDL3 window, keyboard, mouse; `desktop.rs` split into a portable loop | 2 |
| L3 | Full mode renderer on the SDL window's wgpu surface (Vulkan / Metal), HUD and GUI as in the page | 1 |
| L4 | Pads, then wheels with FFB (SDL3); `controls.ini` as on Windows | 1.5 |
| L5 | Timer, physics thread, `/dev/shm` shared memory | 0.5 |
| L6 | FMOD 2.03 native (the `fmod/web.rs` mapping over the C ABI), the required-files list generator, licence text | 1.5 |
| L7 | Launcher on `wry`, folder chooser, check, zip read in place | 2 |
| L8 | Packaging: a release workflow row per target (tar.gz / unsigned `.app` in a zip), proof on Super's Linux | 0.5 |
| M1 | macOS Apple Silicon: Metal limits, app bundle, Gatekeeper ("unsigned: right-click, Open"; signing needs a paid Apple account), the NaN replay check | 2 |
| X1 | Linux ARM + macOS Intel (universal binary) | 0.5 |
| W1 | Windows on ARM: read the emulated row; if `rcpps` differs, the table under emulation | 0.5 |
| Later | YEBIS on wgpu (also brings post-processing to the browser) 2–3; CPython 3.3 builds 2; triple screens on wgpu 0.3; VR via OpenXR (not sized) | 4.5+ |

**Per platform:**

| Platform | First-build features | Player supplies | Sessions |
|---|---|---|---|
| Linux x64 | The browser's list (bit-exact physics, AC's renderer and graphics settings without YEBIS, HUD/menus/apps, AI races, multiplayer, replays) **plus** what a browser can't do: wheels with FFB, `controls.ini`, real files and setups, `/dev/shm` telemetry, no memory cap, any car or track without a reload. No Python apps, VR, TrackIR, Steam, CSP or YEBIS | AC's files (a Proton install is found by itself), `libwebkit2gtk-4.1` and SDL3 from the distro (or SDL3 shipped), Vulkan drivers | ~9.5 (L1–L8) |
| macOS Apple Silicon | Same as Linux, on Metal; wheels best effort; shared memory for native tools only | AC's files copied from a Windows install (no Steam AC on macOS); allowing the unsigned app once | +2 (M1) |
| Linux ARM | Same as Linux x64 | Same | +0.3 (X1) |
| macOS Intel | Same as Apple Silicon (one universal app) | Same | +0.2 (X1) |
| Windows on ARM | **The x64 release as it is**, under Windows' emulation: everything, because AC's FMOD, Python and MSVCR120 DLLs are x64 and load fine in an emulated x64 process. What could break: `rcpps` and the maths path through the emulator (section 4); D3D11 on the ARM GPU driver is ordinary. A native ARM64 build couldn't load those x64 DLLs: no sound, no Python, until FMOD's Windows ARM64 library and the 2.03 mapping (L6) are used | Nothing new | 0.5 (W1) |

## 9. Not done, and open questions

- **Not done:** the workflow hasn't run (Super pushes and starts it). Section 3.3 is the expectation
  from the code. No game code changed, and no cross build was tried locally (the target's C
  toolchains, like ring's, aren't on this PC).
- The base-game size of the required-files list isn't measured (step L6 generates it).
- `actions/upload-artifact@v5`: the repo had no pinned version to copy. If GitHub says the major
  is retired, raise it.
- Questions for Super: (1) SDL3 for window, input and FFB (one C library, zlib licence, shipped or
  from the distro) instead of `winit` + `gilrs` + our own FFB: recommended, because it is the only
  one with wheel FFB on Linux and macOS. (2) Unsigned macOS app for now? (3) Is Super's wheel
  testable under Linux (a live USB is enough)? (4) Once the NaN check is done on a Mac, does a
  difference there count as "the game's" (`differences.md`) or as a bug to fix? A fix would
  canonicalise NaNs at the few places that make them.
