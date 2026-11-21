# Native builds beyond Windows x64 (Task P1, spike)

macOS (Apple Silicon, Intel), Linux (x64, ARM) and Windows on ARM: what stands in the way, what
already gives the same bits, and the order to build it in. A spike: no game code changed, no
version. The only code is the manual workflow `.github/workflows/platforms.yml`.

## Resume here

- P1 done: the inventory (section 2), the workflow (section 3), the bit-exactness section (4), the
  game files, FMOD and Python decisions (5 to 7), the plan (8).
- P2 done (section 10): run 1 read (3.3), the workflow counts skipped goldens, the physics, ODE,
  maths and net goldens are identical on x86 Linux and on ARM under QEMU, the NaN sign audit
  (nothing reads it), and `rustyac-audio` and `rustyac-game --features wgpu` build off Windows.
- Next: Super merges and reruns the workflow (10.5); its rows should match section 3.4. Then P3,
  the native game from section 8's L1, starting with the list in 10.4.

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
`rcpps` instruction it uses Intel's `rcpps` table. Task P2 checked this: every physics, ODE, net
and maths golden gives the game's bits on x86 Linux and on ARM Linux under QEMU, with the real
car and track files. **The sign of a NaN** made by an invalid operation (negative on x86,
positive on ARM) does differ, but nothing in the drive reads it, so only the proofs' compares need
to treat NaNs as equal, which they already do. A replay on a real Apple Silicon Mac is the last
check.

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

### 3.3 Results of the first run (run 44794455197, P1's workflow, read in Task P2)

Logs: `re/scratch/taskP1/run/logs-<target>/`.

| Row | Linux x64 / ARM, macOS ARM / Intel | Windows ARM64 | x64 emulated |
|---|---|---|---|
| `rustyac` | ok (the replay runner) | ok (the full game, D3D11 included) | ok |
| each crate | ok **except** `rustyac-render` (its `compile_error!` without `backend-wgpu`: by design) and `rustyac-web` (unresolved `render::ac`, `gui_apps`, `rustyac_game::audio`: they are Windows / wasm only) | ok | ok |
| `render-wgpu` | ok | ok | ok |
| `game-wgpu` | FAILED: `rustyac_render` not found (the dependency was Windows / wasm only; fixed in P2) | ok | ok |
| tests | ok except `rustyac-audio` (`raw::load` missing in its golden test and `fmod_smoke`; fixed in P2). **But every golden that needs car data or the track returned early after NOT TESTED and passed in 0.00 s**: libtest hides the output of passing tests, so the "NOT TESTED lines" column read 0. No car was stepped on any runner | ok | ok, and `rcpps_tests::the_table_is_the_instruction_on_an_intel_processor` ran and passed: Windows' x64 emulator answers `rcpps` as Intel does |

So the run proved that everything builds on every target apart from the four rows above, and that
the game-free tests pass everywhere. It did **not** prove the physics bit-exact on ARM: Task P2's
step 2 (section 10.2) does that on this PC.

### 3.4 What the next run should show (after Task P2)

The summary's last column is now **tests run / skipped**: tests run with `--show-output`, and a
test that printed NOT TESTED counts as skipped, not as run. Rows that fail by design say
**expected (by design)** and don't fail the job.

| Row | Linux x64 / ARM, macOS ARM / Intel | Windows ARM64 / x64 emulated |
|---|---|---|
| `rustyac`, crates, `render-wgpu` | ok; `rustyac-render` and `rustyac-web` say expected | ok |
| `game-wgpu` | **ok** on Linux x64 (built here, section 10.4); macOS's Metal side of wgpu not tried | ok |
| `test-rustyac-math` | 4 / 1 (the MSVCR120 sample is skipped) | more run: the DLL's tests are Windows only |
| `test-rustyac-ode` | 27 / 0 | the same |
| `test-rustyac-physics` | 107 / 28 (the goldens with car data or a track, section 10.1) | the same |
| `test-rustyac-net` | 19 / 2 (the two Laguna Seca join goldens) | the same |
| `test-rustyac-content`, `-gui` | as in run 1 (not counted here) | the same |
| `test-rustyac-audio` | **ok**, 9 / 1 (the FMOD 1.08 golden) | 9 / 1 (no AC on the runner) |

A Linux or macOS job is green unless something fails that isn't by design. The numbers are this
PC's Linux x64 run with the game files hidden (section 10.1); a macOS row may differ by a few
platform-gated tests.

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

- **The NaN sign on ARM** (settled by Task P2, section 10.3: a platform difference, not a bug).
  An invalid operation (0/0, ∞−∞) gives `0xFFC00000` on x86 and `0x7FC00000` on ARM. AC's states
  do hold NaNs (the AI line's side velocity, `web.md` 3.2), but nothing in the drive reads their
  sign. Comparisons between platforms must count any NaN as equal to any NaN (`replay_hash`'s
  NaN-blind hash, the golden tests' compares). Still to do on real hardware: one replay on an
  Apple Silicon Mac, `rustyac --replay x.ryin --headless --dump-states` against the Windows dump
  (the WASI proof's recipe), compared NaN-blind. The ARM goldens already pass under QEMU (10.2).
- **x64 under emulation** (Windows on ARM's Prism, macOS's Rosetta 2): run 1 showed that Prism
  answers `rcpps` as Intel's table does (the emulated row's ODE test). Rosetta 2 isn't tried yet.
  `MSVCR120`'s FMA3 or SSE2 choice follows the CPUID the emulator reports.

**What CI can prove without the game:** only what is committed. That is the physics goldens
(`crates/rustyac-physics/tests/golden`, 37 files, 4.6 MB: tyre, SCTM, replays.rs), the ODE's
recorded operations (`crates/rustyac-ode/tests/data`, 34 files, 1.2 MB), the maths' recorded
hashes, and the net and audio goldens (176 KB, 140 KB). All of that is already in the repo and
already in the workflow (Task P2 measured it: 157 tests run and 31 skip without the game's files,
section 10.1). **No kept recording can go in the repo.** A replay needs the car's and
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

- **Not done (P1):** no game code changed. Run 1 was read in P2 (section 3.3), and P2 built and
  ran Linux x64 and ARM locally in WSL (section 10.2).
- **Not done (P2):** the rerun of the workflow with P2's changes (Super, 10.5); the made-up whole
  car for runners (10.1, about one session); a replay on a real ARM Mac; the native game itself
  (P3).
- The base-game size of the required-files list isn't measured (step L6 generates it).
- `actions/upload-artifact`: raised from v5 (Node 20) to v7 (Node 24) in P2.
- Questions for Super: (1) SDL3 for window, input and FFB (one C library, zlib licence, shipped or
  from the distro) instead of `winit` + `gilrs` + our own FFB: recommended, because it is the only
  one with wheel FFB on Linux and macOS. (2) Unsigned macOS app for now? (3) Is Super's wheel
  testable under Linux (a live USB is enough)? (4) Once the NaN check is done on a Mac, does a
  difference there count as "the game's" (`differences.md`) or as a bug to fix? A fix would
  canonicalise NaNs at the few places that make them. (Answered by Task P2's audit, section 10.3:
  nothing in the drive reads a new NaN's sign, so it is a platform difference for the compare
  tools.)

## 10. Task P2: what the first run proved, the ARM bits, the cheap build fixes

### 10.1 An honest workflow

- **The blind spot:** a golden that needs car data (`cardata/`) or AC's folder prints NOT TESTED
  and returns, so it passes in 0.00 s. libtest only shows the output of failing tests, so run 1's
  column counted nothing. The tests now run with `-- --show-output`, and the summary counts the
  tests whose output says NOT TESTED as **skipped**, apart from the ones that **ran** (an awk over
  the log, the same as `re/scratch/taskP2/counts.awk`). What passes is unchanged.
- **Rows that fail by design** off Windows (`rustyac-render` without `backend-wgpu`, and
  `rustyac-web`) say "expected (by design)" and don't fail the job. If one starts to build, it
  says ok.
- `actions/upload-artifact@v7` (Node 24; v5 ran on Node 20), the same major as
  `actions/checkout@v7`.
- **What a runner with no game files proves** (measured: the Linux x64 test binaries run again
  with `cardata/` hidden and no `AC_ROOT`, `re/scratch/taskP2/nofiles.sh`): 157 tests run and 31
  skip. The ones that run are all of **ODE**'s recorded operations, functions and car rows (27);
  the **tyre** and **SCTM** goldens, the **replay** save/load/play tests, the **referee** against
  the oracle's approved file and the weekend sessions, three pit-stop and eight chassis tests
  (file read-backs, damper branches) and the physics unit tests (107 in all); **net**'s unit and
  robustness tests (19); the **maths**' recorded sample hash (4). The 31 skipped are every
  whole-car replay: the chassis, track, collision, AWD/ERS and Task 17 goldens, the whole-car
  F2004 tests, several cars, AI, pit stops, the race start, the Laguna Seca join (twice), the
  tyre tester, Spa's AI-line grid, plus the MSVCR120 sample.
- **Could a small car run on a runner?** **Yes, as our own made-up car, never AC's.** The repo
  already does this for tyres (`tests/golden/synthetic_car/`: invented `tyres.ini` and LUTs whose
  golden was recorded from the game's own code). The whole-car tests drive the F2004 on a flat
  road with no track (`whole_car.rs`), so a made-up car (every ini and LUT written by us), driven
  on that flat road in `acs.exe` by the car oracle, would give a whole-car golden with no AC file
  in it. Not done: about one session (the car's files, the oracle recording, the test). AC's own
  cars and tracks: never (rule 7).

### 10.2 The ARM bits, on this PC (WSL + QEMU)

Set-up (inside the WSL distro `Ubuntu` 26.04 only): `build-essential`, `gcc-aarch64-linux-gnu`,
`libc6-dev-arm64-cross`, `qemu-user`, and rustup with 1.97.0 and `aarch64-unknown-linux-gnu`:
2.3 GB in the distro, whose disk is on C:. Sources, `target/` and logs stay in the repo through
`/mnt/c`, in `re/scratch/taskP2/`. `wsl_tests.sh <target>` builds and runs the math, ODE, physics
and net tests with `AC_ROOT` and `cardata/` on the real files (read only). The aarch64 binaries
are aarch64 ELF files and run under `qemu-aarch64 -L /usr/aarch64-linux-gnu`. Run it from Git
Bash as `MSYS_NO_PATHCONV=1 wsl -d Ubuntu -- bash /mnt/c/…/re/scratch/taskP2/wsl_tests.sh
aarch64-unknown-linux-gnu` (without `MSYS_NO_PATHCONV`, Git Bash rewrites the `/mnt/c` path).

| Golden test (steps, from the golden files' names) | x86 Windows | x86 Linux (WSL) | ARM Linux (QEMU) |
|---|---|---|---|
| `golden_ai_race` (3000) | identical, 3.96 s | identical, 10.2 s | identical, 53.6 s |
| `golden_ai_passes_a_car_asleep_on_the_grid` (2400) | identical, 3.31 s | identical, 7.7 s | identical, 41.1 s |
| `golden_pit_stops_on_fed_states` (3600) | identical, 1.69 s | identical, 4.9 s | identical, 32.5 s |
| `golden_race_start_with_the_referee` (2700) | identical, 2.56 s | identical, 7.3 s | identical, 39.7 s |
| `golden_rear_end` (1900) and `one_car_in_the_engine_is_the_car_alone` | identical, 1.53 + 0.39 s | identical, 8.1 + 1.8 s | identical, 31.5 + 6.7 s |
| `golden_laguna_join` and `…_the_browsers_way` (3000 each) | identical, 2.8 s each | identical, 7.7 s each | identical, 39.0 / 39.8 s |
| `the_car_on_spa_matches_the_game` (300 + 200) | identical, 0.67 s | identical, 8.9 s | identical, 16.6 s |
| `the_car_on_magione_matches_the_game` (200 + 300) | identical, 1.18 s | identical, 10.8 s | identical, 23.9 s |
| `the_car_hits_spa_s_walls_and_lands_on_its_roof_like_the_game` (200 + 200 + 340 + 420) | identical, 0.98 s | identical, 11.1 s | identical, 24.0 s |
| `the_awd_car_and_the_ers_car_match_the_game` (300 + 400) | identical, 0.39 s | identical, 6.5 s | identical, 6.0 s |
| `strut_axle_rear_steer_and_old_tyre_match_the_game` (4 × 300) | identical, 0.44 s | identical, 6.9 s | identical, 7.7 s |
| chassis `golden_steps_are_ac_s` (300 + 300 + 460 + 400 + 400) | identical, 0.52 s | identical, 7.4 s | identical, 9.4 s |
| `track::spline` lookup grid (Spa's AI line) | identical, 12.1 s | identical, 10.9 s | identical, 110 s |
| the whole-car F2004 tests, the referee, weekend, tyre, SCTM and ODE goldens | identical | identical | identical |
| `rcpps` is the instruction on an Intel processor | identical | identical | not run (x86 only; ARM uses the table) |
| maths: `sample_matches_msvcr120` | identical (the DLL) | not run (no DLL) | not run (no DLL) |

Totals: Windows 190 run / 0 skipped, Linux x64 187 / 1, ARM 186 / 1. The skips are the MSVCR120
sample, and the totals differ by the Windows-only and x86-only tests. **No golden differs
anywhere**, and none printed NOT TESTED. Linux is slower than Windows because the car and track
files are read through `/mnt/c`.

**What QEMU proves and doesn't.** QEMU runs the real aarch64 machine code the compiler made, with
ARM's floating-point rules done in software. An invalid operation gives ARM's default NaN
(`0x7FC00000`, positive), NaNs propagate by ARM's operand rules, and flush-to-zero is off, as
Linux and macOS leave it. So it proves that the code paths ARM takes (the `rcpps` table, no
`MXCSR`, the pure maths) give the game's bits. Fused multiply-add: Rust never fuses on its own and
our `fma` is software (`libm::fma`), so no `fmadd` is executed either way. What it doesn't prove:
a real core's hardware (QEMU's soft-float models the spec, not the silicon), and macOS's own
runtime. A replay on an Apple Silicon Mac (section 4's `--dump-states` check) confirms it.

**x86 Linux against Windows:** no difference, so no bug to find.

**NaN in the goldens:** the comparisons count any NaN as equal to any NaN (`same_word` in
`engine/record.rs`, `car/replay.rs`'s field compare and hash), and the committed goldens hold
almost no NaN words (three byte matches of a NaN pattern in `chassis_kerb_2684_300.chgold`,
alignment not checked). So step 2 shows that every non-NaN value is identical on ARM. That a NaN's
sign never changes one is section 10.3's audit.

### 10.3 The NaN sign audit

x86 makes `0xFFC00000` from an invalid operation, ARM makes `0x7FC00000`. Only a **new** NaN
differs: a NaN that is passed through keeps its sign on both. I searched physics, ODE, net, maths,
replays and the proofs for `copysign`, `signum`, `is_sign_*`, `total_cmp`, `to_bits`/`from_bits`
on floats, sign-bit masks, and float bits compared as integers.

| Hit | Can a NaN reach it? | Would the sign change a result? |
|---|---|---|
| `race/output.rs` `stream_float` (`-1.#IND` vs `1.#QNAN`) | No known source: it prints mode scores, speeds and times from finite state, and pit presets read from files | Yes, the text, if one ever got there |
| `rustyac-ode` `collision.rs` ray/triangle sign-bit tests of `u`, `v`, `dist` | Only from a NaN ray origin (a car already at NaN): the mesh and the direction are finite | Yes then (hit or no hit), but that drive is already lost |
| `odemath.rs` `safe_normalize3` `copysign`; the same in `csp_core.rs` | No: `a[idx]` is the largest component and never NaN (a NaN first component takes the zero-vector branch) | — |
| `dxmath.rs` `xm_atan2` sign bits | In `xm_quaternion_slerp` (net cars, replays, ghosts): `sin_omega = sqrt(1 - cos²)` is NaN only when cos > 1, and then the linear branch throws `omega` away | No |
| `dxmath.rs` `xm_sin` sign masks | Only from a NaN angle, whose result is NaN either way | Only the NaN's own sign |
| `track/spline.rs` `side_velocity` (0/0 every step: the game's post-step divides by a zero time step) | **Yes, every step** | No: it is only stored, traced and saved. Nothing reads it |
| `world.rs` body moved (bits), `chassis.rs` ray cache (bits), `collide_ttl.rs` contact key (bits) | Only from a NaN body position | No in practice: both sides come from the same machine |
| `replay/half.rs`, the state savers, net packets, `referee.rs`/`pitstop.rs` proof lines | Yes: `side_velocity` in a trace or a state | Only in the saved or printed bits, never in the drive |
| CSP: `csp_lcp.rs` `copysign(∞, -w)`, `csp_space.rs` `is_sign_negative` | CSP is a Windows x64 DLL: never on ARM | — |
| `total_cmp` sorts in `track_info.rs`, the desktop and the renderer | Tools and the D3D11 debug view, not the physics | — |

**Decision: a platform difference, not a bug.** Nothing in the drive reads a new NaN's sign, so
proofs that compare different platforms treat any two NaNs as equal, in the **compare tools only,
never the physics**. The golden tests already do (above), and `replay_hash` has its NaN-blind
second hash. The Mac check of section 4 must use that hash (or a NaN-blind dump compare), not the
plain one. No code change: nothing reachable needs canonicalising.

### 10.4 The cheap build fixes

- `rustyac-audio`: its golden test and the `fmod_smoke` example are gated like `raw::load`
  (Windows and wasm). Elsewhere the test prints NOT TESTED and the example says it needs FMOD
  1.08. The loader's private `keep` got the same gate (it was dead code off Windows). `cargo test
  -p rustyac-audio` passes on Linux x64 (9 run / 1 skipped) and clippy is clean.
- `rustyac-game --features wgpu`: `rustyac-render` is now an optional dependency off Windows and
  the browser, turned on by `wgpu`. **The library and `rustyac` (the replay runner) then build on
  Linux x64 with no error**, clippy clean, with the wgpu stack and `render::ac` compiled. One lib
  test line (`usable`, Windows only) got its `cfg`. The lock file is unchanged.
- **The next layer (for P3):** `cargo clippy -p rustyac-game --features wgpu --all-targets` off
  Windows has 6 errors in 4 targets, all of one kind: they import modules that are still
  `cfg(windows)`. They are `examples/achievements_check.rs` (`achievements`, `steam`),
  `examples/steam_check.rs` (`steam`), `tests/replay.rs` (`shm`, twice) and
  `tests/write_guard.rs` (`cli`). They are compile errors only in tests and examples. The real next
  layer is the gate itself: the window, the loop, input, sound, shared memory, the CLI and the
  rest of `lib.rs`'s `cfg(windows)` modules, which is section 8's L1–L6. Not started (P3).
- `rustyac-web` for a native target: **excluded** (expected by design). It is the wasm crate, and
  its desktop build only exists for the Windows desktop's tests of its plain logic. What it lacks
  off Windows (`render::ac` without `wgpu`, `gui_apps`, `rustyac_game::audio`) is exactly what P3
  ports, so gating it now would only move that code around. Revisit once the native game has
  sound.

### 10.5 Steps for Super

1. Merge `web` into `master` and push.
2. `gh workflow run platforms.yml --ref master`, then `gh run watch`.
3. `gh run download <run id> --dir re/scratch/taskP2/run` and give the next session the run id.
4. What it should show is section 3.4. A Linux or macOS job going red means a row failed that
   isn't by design: `game-wgpu` on macOS is the one not tried here.

### 10.6 Checks run for P2

- Windows x64: `cargo test --release -p rustyac-math -p rustyac-ode -p rustyac-physics -p
  rustyac-net` with the game files (190 run, 0 skipped); clippy `-D warnings --all-targets` on
  `rustyac-audio` and `rustyac-game`, with and without `wgpu`.
- Linux x64 (WSL) and ARM (QEMU): the same tests (section 10.2); Linux x64 also ran `cargo test -p
  rustyac-audio` and clippy on `rustyac-audio` and on `rustyac-game --features wgpu` (lib and bins).
- Skipped: `run_all.sh --quick`, because the Windows build compiles the same code as before (the
  audio and game changes are `cfg` gates for other targets, and the Windows dependency graph and
  lock file are unchanged), so it can't break what the quick lane proves. Wasm clippy also skipped:
  the wasm side compiles the same code as before (the gates include `wasm32`).
