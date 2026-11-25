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
- **P3 (Task P3a, branch `web`): done (section 11).** L1 (11.2), L2 + L3 (11.3, 11.4), the frame
  check against the page's Full mode, the physics dumps and quitting (11.5).
- **P3b (branch `web`), section 12:** L4 done (12.1: pads, wheels, force feedback on SDL3, proved
  against kernel devices made with `uinput`). L5 done (12.2: `/dev/shm`, pages byte-identical to
  Windows'). L6's sound done (12.3: FMOD Engine 2.03 native, call logs identical to the page's on
  the 18 drives and to the game's on the 3 end-to-end rows), its licence text and required-files
  list too, split into the base game and one list per DLC (12.3). **P3b done**; next is L7 (12.4).
- **P3c (branch `web`), section 13:** L7 parts 1 and 2 committed (13.1: imports, zip, the lists'
  checks; 13.2: the launcher on wry with the chooser, built, not yet proved: the page stalls in WSL).
  **Resume at 13.3** (L7's proof, then L8).

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
   21.4 GB, 26 tracks 10.3 GB, mods and CSP included). The lists (step L6, section 12.3):
   **base 28.6 GB** (19,714 files), **everything 34.3 GB** (21,372 files).
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
| Later | YEBIS on wgpu (also brings post-processing to the browser) 2–3; CPython 3.3 builds 2; triple screens on wgpu 0.3; VR via OpenXR (not sized); kgl on wgpu's GL backend 0.5 (non-filtering samplers for float textures without `FLOAT32_FILTERABLE`, one sampler per texture in a shader: only for WSLg's GPU and cards without Vulkan, section 11.3) | 5+ |

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
- The base-game size of the required-files list: measured in L6 (section 12.3), 28.6 GB.
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

## 11. Task P3: the native Linux game (L1 to L3)

### 11.1 Where it runs

WSL's distro `Ubuntu` on this PC is **Ubuntu 26.04.1** (P2 upgraded it; the prompt says 24.04).
Packages go inside the distro; sources, `target/` and logs stay in the repo through `/mnt/c`.
`re/scratch/taskP3/wsl.sh <log> <cmd...>` runs a command there with P2's toolchain (1.97.0) and
P2's target folder (`re/scratch/taskP2/target-linux`, reused so nothing builds twice). P3 added
the two wasm targets to WSL's 1.97.0 (this PC's Windows Rust has none, so wasm clippy runs there).

What WSLg offers a window (checked, nothing installed for it yet): Wayland and X11 (`wayland-0`,
`:0`); SDL3 is in the archive (`libsdl3-dev` 3.4.2); Mesa 26.0.8's Vulkan drivers have **no
`dzn`** (Vulkan on D3D12), so Vulkan is only lavapipe (the CPU); the GPU is reached through
Mesa's d3d12 driver for **OpenGL** (WSLg's `/usr/lib/wsl/lib/libd3d12.so`). So wgpu gets GL (EGL)
on the GPU, or Vulkan on the CPU. Which one works for kgl's shaders is L3's first check.

### 11.2 L1: the portable modules (done)

- **Ungated off Windows:** `cli`, `conditions`, `identity`, `join`, `gateway`, `steam`,
  `achievements` (native targets only: both wasm builds keep the modules they had), and `audio`
  (now every native target too).
  `ai_record` stays Windows: it is a physics-thread sink (L5).
- **TLS:** `rustls` and `webpki-roots` are declared for every native target (they were Windows
  only); `rustyac-audio` too (sound off Windows comes with L6). The lock file is unchanged.
- **Steam off Windows:** `steam::load` and `achievements::Flat::loaded` answer "no Steam", so the
  rest of both modules (the achievement rules, the stand-in Steam of `steam_check`) builds and
  runs everywhere. `identity::default_name` reads `USER` off Windows (`USERNAME` on Windows).
- **FMOD off Windows:** `rustyac-audio`'s `raw::load` says "no FMOD on this platform yet (step
  L6)"; the drive goes on without sound, as when the DLLs are missing on Windows.
- **Paths** (`rustyac-content::install`): the game's folder inside a library is joined one part at
  a time (a backslash is no separator off Windows; on Windows the string is the same). Off
  Windows Steam's folders are `~/.local/share/Steam`, `~/.steam/steam` and Flatpak's
  `~/.var/app/com.valvesoftware.Steam/.local/share/Steam`, each read through the existing
  `libraryfolders.vdf` parser (a test now holds a Linux file too). `Documents\Assetto Corsa` off
  Windows is the one in **Proton's prefix** beside the library AC is in
  (`steamapps/compatdata/244210/pfx/drive_c/users/steamuser/Documents/Assetto Corsa`), else
  `~/Documents/Assetto Corsa`. The write guard (`guard.rs`) uses the same function.
- **A guard hole found on Linux and closed:** `std::path::absolute` keeps `..` off Windows (it
  folds it on Windows), so `resolved` met a last part with no file name and let the path through.
  It now keeps the `..` and folds it itself (`documents_refused_elsewhere_allowed` caught it on
  Linux). On Windows the path never ends in `..` there, so nothing changes.
- **Tests and examples:** `the_static_page_holds_the_hybrid_as_built` is Windows until `shm` (L5);
  `mirror_check` needs `--features wgpu` off Windows; `tls_front` runs on any native target.
- Without `wgpu`, `conditions` and `audio` skip `render::gui_apps` off Windows (the replay
  runner build has no renderer).

Proofs (logs in `re/scratch/taskP3/`):

| Check | Result |
|---|---|
| Linux x64: `cargo build --locked --workspace --all-targets --exclude rustyac-web --exclude rustyac-render` (the two by-design rows of 10.4) | ok (`l1_ws6.log`) |
| Linux x64: clippy `-D warnings --all-targets`, `rustyac-game` (with and without `wgpu`), `rustyac-content`, `rustyac-audio` | clean |
| Linux x64: `cargo test --release` of those three (with `wgpu`, the real AC files read through `/mnt/c`) | all pass (`l1_test2.log`) |
| wasm32-unknown-unknown: clippy `-D warnings --all-targets -p rustyac-web` | clean |
| wasm32-wasip1: clippy `-D warnings` lib and bins of `rustyac-game`, `rustyac-content` | clean (`--all-targets` fails as before P3: `write_guard.rs` imports `cli`) |
| Windows: clippy `-D warnings --all-targets`, the three crates, with and without `wgpu` | clean |
| Windows: `run_all.sh --quick`, every area, 2 lanes (10.3 GB free RAM) | **258 cases IDENTICAL** in 14 areas, 0 different, 0 errors, csp's 1 expected skip; 1h05m (`quick_l1.log`) |

### 11.3 L2 + L3 steps 1 to 3: the game on Linux (built, committed as one piece)

The plan was three commits; they came as one, because the loop, the timer and the surface only
run together (the window needs the surface to draw the first frame, the drive needs the timer).

- **`desktop.rs` builds for `any(all(windows, not(wgpu)), all(linux, wgpu))`.** On Windows its
  code paths are as they were: the cfg picks a file, and the few Win32 calls the loop made itself
  (`GetAsyncKeyState`, `GetCursorPos`, `SetCursorPos`, `GetWindowRect`, the screen's size) are
  now `window::key_async` and friends, which make the same calls in `window.rs`.
- **The platform layer, Linux** (`#[path]` picks the file, the Windows file is untouched):
  `window_sdl.rs` (the same `Event`s, the virtual-key codes `controls.ini` binds made from SDL's
  scancodes, keys and the cursor in atomics for the physics thread, `WM_CHAR`'s control
  characters, Win32's `ShowCursor` counter for `mouse_hider.rs`), `timer_unix.rs` (sleep, then a
  250 us spin; no raised priority without `CAP_SYS_NICE`), `input/dinput_none.rs` and a
  non-Windows `XInput` (no pads or wheels: L4), `shm_none.rs` (no pages: L5). TrackIR, Python,
  VR, the power request and the debug view (`--debug-view`, D3D11)
  are Windows only; SDL keeps the screen saver off by itself. (Ctrl+C: 11.5.)
- **Opened to Linux** (their code unchanged): `physics_thread`, `live`, `wheel`, `mouse_hider`,
  `ai_record`, `audio::AudioSink`, `Bindings::load`. `render::picture` (`box_downscale` and
  `write_png` moved there from `render/mod.rs`, which re-exports them) has a debug view that
  cannot be made on Linux, so its arms stay as they are.
- **The surface** (`Window::create`): a wgpu instance with SDL's display handle (GL needs it),
  a surface from SDL's raw handles, an adapter compatible with it, the device given to kgl
  (`AcRenderer::give_device`) before the renderer is made; `AcRenderer::swap_chain` /
  `resize_swap_chain` / `present` on Linux take that surface (`present_to`, as the page does).
  `WGPU_BACKEND` (`vulkan`, `gl`) is read by rustyAC (wgpu's own reading needs its `std`
  feature, which is off).
- **SDL3: built from source and linked statically** (`sdl3` 0.22 with `build-from-source-static`,
  SDL 3.4.20): the player needs no SDL package. The build needs CMake, a C compiler and the X11 /
  Wayland headers (in WSL: `cmake ninja-build pkg-config libx11-dev libxext-dev libxrandr-dev
  libxcursor-dev libxi-dev libxss-dev libxfixes-dev libxtst-dev libxkbcommon-dev libwayland-dev
  wayland-protocols libegl-dev libgl-dev libdecor-0-dev libdrm-dev libgbm-dev libudev-dev
  libdbus-1-dev libasound2-dev libpulse-dev`); SDL loads X11 / Wayland at run time. What a player
  needs: an X11 or Wayland desktop, Vulkan drivers (Mesa's or the GPU maker's), and for L7's
  launcher `libwebkit2gtk-4.1`. Licences: `LICENSING.md` (SDL's row, 13 new crates in the list)
  and `tools/notices.py` (SDL's Zlib text with its copyright).

**What wgpu gets in WSLg** (Ubuntu 26.04.1, Mesa 26.0.8, SDL picks X11 there):

| Backend | Adapter | Result |
|---|---|---|
| Vulkan (the default pick) | llvmpipe (lavapipe, the CPU) | **right**: 0 wgpu errors; 1.9 FPS at 1280 x 720 (Spa, the F2004, autodrive) |
| GL (`WGPU_BACKEND=gl`) | llvmpipe through EGL (no DRI3 on WSLg's X server) | 461 validation errors in 30 s, 7.2 FPS |
| GL with `GALLIUM_DRIVER=d3d12` | **D3D12 (AMD Radeon Pro 5500M)**, GL 4.6: the GPU | the same validation errors, 7.2 FPS |

kgl on wgpu's GL backend breaks two GL limits: AC's float targets are sampled with filtering
samplers (GL adapters have no `FLOAT32_FILTERABLE`: "Sampler binding N expects filtering =
false"), and one texture is read with two samplers in one shader (GLSL's combined samplers
cannot: "A image was used with multiple samplers"). Real Linux has Vulkan on the GPU, so this
only matters for WSLg; as the lead chat allowed, L3 is proved on lavapipe and a GPU run waits
for L8's live USB. A kgl fix for GL is in "Not done". (Wayland, `SDL_VIDEO_DRIVER=wayland`:
the GL run crashed on exit, exit code 139: found and fixed in 11.5. X11 is SDL's pick on WSLg.)

### 11.4 Proofs of 11.3

| Check | Result |
|---|---|
| Windows: `run_all.sh --quick render gui fresh` (one lane, 10.5 GB free RAM) | **110 cases IDENTICAL** (render 18, gui 29, fresh 63), 0 different, 0 errors; 33 min (`re/scratch/taskP3/quick_l2.log`) |
| Windows: clippy `-D warnings --all-targets -p rustyac-game`, with and without `wgpu` | clean |
| Linux: clippy `-D warnings --all-targets -p rustyac-game`, with and without `wgpu` | clean |
| Linux: `cargo test -p rustyac-game --features wgpu --lib window` (scancodes to virtual keys, Ctrl's sides) | pass |
| Linux, WSLg: `--autodrive` at Spa, 90 s (`run/gl60`) | physics 30,060 steps at 333.33 Hz; 169 frames, 1.9 FPS on lavapipe; clean end |
| Linux, WSLg: real X11 keys from `xdotool` (`keys.sh keys2`): Up held 8 s, F8, Esc | the car drove about 190 m on the Up key; F8 wrote the game's own picture (right colours); Esc opened AC's pause menu (Resume, Back to pits, Restart Session, Replay, Exit); the end at `--duration`: exit 0, the usual report |
| Linux, WSLg: the same with a click on the menu's Exit (`keys.sh keys6`) | the program ended at once, exit 0, the usual report |
| wasm32-unknown-unknown clippy `-p rustyac-web --all-targets`; wasm32-wasip1 clippy lib + bins `rustyac-game`, `rustyac-content` | clean |

Pictures: `re/scratch/taskP3/run/keys6/2_esc.png` (the pause menu over Spa, grabbed with
ImageMagick's `import`), `run/keys2/screenshots/` (the game's own F8 picture). The grabs in
`run/keys2/*.png` have wrong colours: `xwdtopnm` reads WSLg's 32-bit visual as (G, R, alpha).
Focus: WSLg's window manager has no `_NET_ACTIVE_WINDOW`; `xdotool windowfocus` gives the window
the keyboard, but only while nobody is using the desktop (runs keys3 to keys5 got none: the
physics stayed paused, as the game does without focus). One drive by hand ended with its report
and then sat until the 300 s watchdog of `run.sh` killed it (exit 124, `run/super_drive`); it
was not seen again (keys2 at `--duration`, keys6 at Exit: exit 0).

### 11.5 P3a's end: the frame check, the physics dumps, quitting (done)

**L3's frames against the browser's Full mode.** The `browser` area compares physics only, so the
check is Task 38i's (`docs/port/web_renderer.md`, "Step 4"): the page's fixed frame (`full.rs`, a
drive as it starts, 3 frames of 1/60 s, 1280 x 720, HUD hidden, kgl's call log from the
renderer's making to the last frame), run by the `full_frame` example. 38i showed that the page's
log equals native wgpu's and native Direct3D 11's (Spa: 17,409 lines). `full_frame` now builds on
Linux (`rustyac-web --features wgpu`: `full` and `rustyac-render` open to it, as `rustyac-game`
takes the renderer there; the lock file is unchanged), so the Linux log is compared with Windows'
Direct3D 11 one, which stands for the page through 38i's chain. The game's own frames after a
drive (`--screenshot`) are compared as pictures: in the game `--gpu-log` is Direct3D 11's own
log (`gpulog`, below kgl), so it has no kgl log to set beside Linux's.

| Frame (1280 x 720) | kgl log, Linux (wgpu, lavapipe) vs Windows (Direct3D 11) | pixels vs Windows on the GPU (Radeon Pro 5500M) |
|---|---|---|
| fixed frame, ks_ferrari_f2004 at Spa from the pit | **IDENTICAL** (17,409 lines, the page's count in 38i) | 39.75 dB, 0.48 % off by more than 8 |
| fixed frame, bmw_m3_e30 at Laguna Seca from the pit | **IDENTICAL** (17,898 lines) | 35.84 dB, 1.70 % |
| the game, `--replay spa_autodrive_300s.ryin --at 20 --screenshot` (269.1 km/h on both) | (none, above) | 28.60 dB, 11.7 % (the road's and grass's texture detail at speed) |
| the game, `--track ks_laguna_seca --car bmw_m3_e30 --autodrive --at 8 --screenshot` | (none) | 30.61 dB, 4.1 % |

The picture differences are the two rasterizers' (lavapipe against AMD's driver; 38i measured
31–35 dB between the page and WARP). Side by side pictures (Windows left, Linux right):
`re/scratch/taskP3/run/ff/*_win_linux.png`, `run/shots/*_win_linux.png`; the window with AC's
pause menu: `run/keys6/2_esc.png`.

**Frame time (release build, lavapipe, 1280 x 720, Spa, the F2004):** 2.1 FPS in the window
(42 frames in 20.1 s, slowest 518 ms; `run/rel_wm`); the fixed frame's 3 frames with loading,
26 s (Windows on the GPU: 8 s). A GPU's numbers come with L8 (Vulkan on real Linux).

**The physics from the Linux game build.** The dumps area's quick drives, `rustyac --replay
<drive> --headless --dump-states -` by the Linux release game build (`--features wgpu`, on copies
of the drives whose AC folder is `/mnt/c/...`, as `drive()` rewrites them for the farm) and by
Windows' `rustyac.exe`: **6 of 6 byte-identical** dumps (sha256; car_random,
conditions_spa_cold_green_wind, t17 kerb strike, t22b crash, t18 Laguna Seca pit, the 300 s Spa
autodrive). Both x86, so no NaN-blind compare was needed.

**Quitting** (`re/scratch/taskP3/close.sh <name> wm|int|term`, `close.py` sends the close
button's `WM_DELETE_WINDOW`; `head_int.sh` for a headless run):

| How | Result |
|---|---|
| the window's close button (debug twice, release once) | the end report, exit 0 |
| Ctrl+C (SIGINT) in a window drive | the end report, exit 0 (before the fix too: SDL turned it into its quit event) |
| Ctrl+C in a headless drive | **the end report, exit 0** (before: the process died at once) |
| the GL run under Wayland | **exit 0** (before: SIGSEGV after the end report) |

- **Ctrl+C:** a SIGINT / SIGTERM handler (`desktop.rs`, `cfg(unix)`) sets the `STOP` flag that
  Windows' console handler sets, so every loop ends the same tidy way. A second Ctrl+C ends the
  process at once (the handler puts the default back). Set before SDL starts, so SDL leaves the
  two signals alone.
- **The Wayland crash, found:** gdb's stack (`run/wl_gl1/out.txt`): `eglTerminate` in wgpu's GL
  instance's drop, inside `libwayland-client`, after `Window`'s drop had shut SDL down and closed
  its Wayland display. wgpu's instance holds that display and goes last (the renderer's buffers,
  kgl's given device). `window_sdl.rs` now never shuts SDL's video down (the window itself is
  still destroyed), so the display stays open until the process ends: exit 0 under gdb.
- **The hand drive that sat after its report (11.4), narrowed:** it hung after the last line
  `run_window` prints, so in the teardown after it (the renderer's and kgl's wgpu objects, then
  the process's end), the same place as the Wayland crash. On X11 with Vulkan, lavapipe's
  surface code also talks to SDL's display. Not seen in this session's 8 ends (the close button 3 times,
  Ctrl+C in a window twice and headless once, `--duration` twice under Wayland GL). If it comes back, `close.sh` prints the
  command for every thread's stack (`wsl -u root gdb -p <pid>`, as ptrace_scope is 1).

### 11.6 Resume here (P3a)

- P3a is done: L1 (11.2), L2 + L3 (11.3, 11.4), the frame check, the physics dumps and quitting
  (11.5). Next: L4 (pads, wheels with FFB on SDL3).
- Tools: `re/scratch/taskP3/run.sh <name> <args>` (a windowed run with its own cfg and Documents),
  `keys.sh` (real keys), `close.sh` (quit tests), `ff.sh` / `shots.sh` + `frames.py` (the frame
  check), `dumps.sh` (the physics dumps), `wsl.sh` (builds). Builds:
  `re/scratch/taskP2/target-linux/{debug,release}/rustyac`, `release/examples/full_frame`.
- Not done, for later: kgl on wgpu's GL backend (section 8's "Later"); the window grabs' colours
  (a tool's fault, 11.4); the dumps of all 228 drives (only the quick six ran).

## 12. Task P3b: pads, wheels, shared memory, sound (L4 to L6)

### 12.1 L4: pads and wheels with force feedback (SDL3)

The Windows files are untouched. Linux gets the same two device layers that AC's classes sit on,
built on SDL3 (already linked statically since P3a), so `pad.rs`'s `JoypadCarControl` and
`wheel.rs`'s `DiCarControl` (the ported arithmetic: dead zones, curves, gamma, filters) run
unchanged on the same numbers as on Windows.

- **Pads** (`input/xinput_sdl.rs`, `pad::XInput` on Linux): the first SDL gamepad that is not a
  wheel, turned back into the `XINPUT_GAMEPAD` Windows reads. SDL defines its values from
  XInput's in its own XInput backend (`SDL_xinputjoystick.c`: the sticks' y as `~y`, a trigger
  byte `b` as `b * 257 - 32768`, then the gamepad's 0..32767); `xinput_sdl::pad_state` undoes
  exactly that. Linux's `xpad` driver reports the same (`~y`, triggers 0..255, which SDL's evdev
  scaling turns into the same `b * 257 - 32768`). Buttons by position (south = A ...). Rumble:
  `SDL_RumbleGamepad` with XInput's two motor words (`pad::motor_word`), restarted on each send
  (XInput keeps a level until the next call; SDL's lasts its duration, 65 s).
- **Wheels** (`input/dinput_sdl.rs`, replacing P3a's `dinput_none.rs`): DirectInput's calls
  (`open`, `poll`, `state`, `enable_ff`, `send_ff`, the device list) on SDL's joysticks and haptics.
  - Values: SDL scales an evdev axis from the device's range to -32768..32767 in float
    (`AxisCorrect`); the raw value is found again from the range (read from the device,
    `EVIOCGABS`) and mapped to DirectInput's -10000..10000 as `MulDiv` does it (Wine's
    `joystick_map_axis`; Windows' own rounding is the one thing not measured: below).
  - Force: one constant force on SDL's steering axis (`SDL_HAPTIC_STEERING_AXIS`, Linux direction
    0x4000), endless, the wheel's auto-centre off (AC's `DIPROPAUTOCENTER_OFF`), capped at 30 % as
    on Windows. The level sent is the smallest that SDL's own DirectInput backend turns back into
    DirectInput's magnitude (`level * 10000 / 0x7fff`), so both systems mean the same force.
  - Not as on Windows: no exclusive access (evdev has none for force feedback); a device
    unplugged and plugged in again is found at the next start only.

**How a Windows `controls.ini` maps on Linux** (read and written as on Windows; `rustyAC`
writes only its own `rustyac_controls.ini`, which on Linux keeps `[CONTROLLERS]` too):

| Key | Windows (acs.exe) | Linux (rustyAC) |
|---|---|---|
| `JOY=n` | the n-th device of DirectInput's enumeration (the game never reads `[CONTROLLERS]`; CSP re-sorts by it) | the device whose product GUID is `[CONTROLLERS] PGUIDn`, built from SDL's vendor and product ids (`C24F046D-0000-0000-0000-504944564944` is a G29); a listed device that is missing keeps its number, empty ("not connected"); unlisted devices follow in SDL's order. No `[CONTROLLERS]`: SDL's order. `__IGUIDn` (instance GUIDs) and `CONn` (Windows' names) are not used: neither exists on Linux |
| `AXLE=k` | `DIJOYSTATE2`'s slot: X, Y, Z, Rx, Ry, Rz, slider 0, slider 1 | the same slot: Linux's `hid-input` gives HID's X..Rz, Slider and Dial the evdev codes 0..7 in that order; SDL numbers only the axes a device has, so the codes come from the device (`EVIOCGBIT`). Other codes (`ABS_WHEEL`, `ABS_GAS`...) get no slot |
| `BUTTON=b` | DirectInput's button b | SDL's button b (evdev's key codes in order from `BTN_JOYSTICK`, as `hid-input` numbers HID buttons) |
| `INPUT_METHOD=X360`, `[X360]` | XInput pad 0 | the first SDL gamepad that is not a wheel; an Xbox-layout pad is listed under its `JOY` number but read as the pad, as Windows does with XInput's twin |

**Proof in WSL** (`re/scratch/taskP3/`): WSL has no pads, but its kernel has `uinput` and evdev
as modules. `fakedev.py` (as root) makes two kernel devices, an Xbox 360 pad as `xpad` reports one
(045e:028e) and a Logitech G29 (046d:c24f) with force feedback (`FF_CONSTANT`, `FF_GAIN`,
`FF_AUTOCENTER`) and axes X (0..65535), Z and Rz (0..255), a 10-bit slider and a hat, but **no Y**
(so the slot map is tested), sets them to raw values and sends the same values to the example
`linux_input_check` (`l4run.sh`). That goes further than SDL's virtual joystick, which the prompt
named: the virtual joystick skips the kernel, SDL's evdev code and its pad mappings, and has no
haptic, so force feedback could not have come out of it. The example reads the devices through the
Linux path and computes Windows' (XInput's raw state as it is; DirectInput's value of the raw
value), runs both through AC's classes and compares; the script answers the kernel's force
uploads and compares each with DirectInput's magnitude. Bindings: AC's defaults, `[CONTROLLERS]`
with the pad as JOY 0 and the wheel as JOY 1, while SDL lists the wheel first.

| Check | Result |
|---|---|
| unit tests (Linux): every XInput stick value and trigger byte back through SDL's numbers; every raw value of the ranges 0..255, 0..1023, 0..4095, 0..16383, 0..65535, -32768..32767, -511..512 through SDL's float scaling to DirectInput's value; every magnitude -10000..10000 back out of SDL's DirectInput backend; the `[CONTROLLERS]` order; the slot map | pass (`l4_b2.log`) |
| the device list | JOY 0 the pad (read as the pad), JOY 1 the G29 (steering wheel, force feedback, steers), as `[CONTROLLERS]` says |
| 400 pad states (random, the dead zone's edges, the ends), `JoypadCarControl` on both | **400 IDENTICAL** (`l4_run2.log`) |
| 400 wheel states (all four axes, hat, 16 buttons), DirectInput's state and `DiCarControl` on both | **400 IDENTICAL** (before the range fix: 32 one-unit misses on the 10-bit slider, `l4_run1.log`) |
| 53 forces (0, ±1, ±1.5, NaN, tiny, random), what reached the kernel vs DirectInput's magnitude | **53 IDENTICAL**: constant force, direction 0x4000, endless, level back to the magnitude; unchanged magnitudes send nothing, as on Windows |
| 6 rumbles (0, 1, over 1, below 0, NaN) | **6 IDENTICAL**: the kernel's strong / weak = XInput's motor words |
| the wheel's set-up | the kernel saw gain 100 % and auto-centre 0 |
| clippy `-D warnings --all-targets -p rustyac-game` (Linux with `wgpu`; Windows with and without); wasm32 clippy (`rustyac-web` all targets, wasip1 lib + bins) | clean |

**Needs real hardware (L8):** a wheel's force direction (SDL's 0x4000 with a positive level is the
convention, but each kernel driver turns it into its own motor sign: a real wheel must pull the way
the car does); Windows' DirectInput rounding of a device range to ±10000 (the MulDiv model is
Wine's: read one wheel's raw values on both systems); pedal and wheel ranges and the axis codes
real drivers give (Logitech's `hid-logitech`, Fanatec's `hid-fanatecff`, Moza and Simagic on
`hid-pidff`); hot-plugging; SDL's mapping for pads other than Xbox ones (a DualShock is a gamepad on
Linux but a plain DirectInput device on Windows, where AC can't use it as the pad).

### 12.2 L5: the shared memory in `/dev/shm`

`shm.rs` now builds on Linux too: its pages, writers and timing are the same code. Only its two
Windows calls have a Linux twin (`shm_unix.rs`); the Windows lines are as they were (only
`cfg(windows)` added above them), and P3a's `shm_none.rs` is gone.

- **Names and layout:** `Local\acpmf_physics`, `acpmf_graphics`, `acpmf_static` become POSIX
  shared memory `/acpmf_physics` ... (`shm_open`), the files `/dev/shm/acpmf_physics` ..., with
  the same bytes. Each file is whole 4096-byte pages, as Windows rounds a mapping (a reader may
  map its own, longer struct). Mode 0600, the user's own, as Windows' default for a session's
  objects.
- **After the drive:** the files stay, with `status` = `AC_OFF`, as a Windows object stays while a
  reader holds it; the next drive writes into the same files (the console says "still held open"
  then).
- **Two writers:** rustyAC refuses to publish while `acs.exe` (under Proton or Wine the process is
  still called that, `/proc/<pid>/comm`) or another `rustyac` runs, as on Windows.
- **Who reads it on Linux:** tools that read AC's layout from `/dev/shm/acpmf_*`: Spacefreak18's
  `simapi` / `simd` (it makes those files for AC under Proton, through `simshmbridge`, which copies
  the Wine mapping out to `/dev/shm`; its docs check AC by `hexdump /dev/shm/acpmf_physics`) and
  `wine-linux-shm-adapter`. With rustyAC native, no bridge is needed: rustyAC writes the files
  those tools read. SimHub itself is Windows only. AC's Python apps that open the pages by their
  Windows name (`sim_info.py`) do not run on Linux anyway (section 7).

| Check | Result |
|---|---|
| Linux unit test: a page made, written, made again (`existed`), 4096 bytes in `/dev/shm` | pass |
| Linux: `the_static_page_holds_the_hybrid_as_built` (was Windows only until L5) | pass |
| the same recorded drives (`car_random`, `conditions_spa_cold_green_wind`), `--replay <drive> --headless --realtime` (the paced loop with every sink); Windows: a script holds the three named mappings (as a reader would) and dumps them after the drive (`shm_win.ps1`); Linux: `/dev/shm/acpmf_*` copied after the drive (`shm_linux.sh`) | **6 of 6 pages byte-identical** (4096 bytes each; 68 to 436 non-zero bytes: the physics page at the last step, the graphics page at `AC_OFF`, the static page) (`run/shm/`) |
| clippy `-D warnings --all-targets -p rustyac-game` (Linux with `wgpu`; Windows with and without) | clean |

A third drive (Laguna Seca's pit, t18) could not run on Windows: Super started Assetto Corsa
during the runs, and rustyAC refused to publish next to it, as it should. (The second drive's run
ended two seconds after AC started; AC writes its own pages every frame and its static page after
loading, so nothing of rustyAC's was left in them.)

### 12.3 L6: sound, the licence, the required-files list

**FMOD Engine 2.03 for Linux (done).** The library is Firelight's `fmodstudioapi20315linux.tar.gz`
(fmod.com, FMOD Engine 2.03.15, Linux), unpacked by Super in `ignored/fmod/fmodstudioapi20315linux/`
(`api/core/lib/x86_64/libfmod.so.14`, `api/studio/lib/x86_64/libfmodstudio.so.14`). The game looks
for both next to its program (the Linux zip, L7), then where the system's loader finds them
(`LD_LIBRARY_PATH` in the proofs). `crates/rustyac-audio/src/fmod/native.rs` binds `raw`'s 1.08
functions to 2.03's C interface (`libloading`, a new dependency of `rustyac-audio` off Windows,
already in the lock file through other crates): 50 of the 65 have a 2.03 C function with the same
arguments (`getLowLevelSystem` is 2.03's `getCoreSystem`), and 15 are translated as `web/sound.js`
translates them for the browser (the version answer 0x10812; `setParameterValue` to
`setParameterByName`; `getMaximumDistance` to `getMinMaxDistance`; the listener with no
attenuation position and 1.08's default before the first set; `Studio::System::create` with
2.03's header version; both advanced-settings structures; `getCPUUsage`'s two structures as
1.08's one; and the `bool` arguments as `FMOD_BOOL`). Unlike the browser's build, the native one has
`Geometry::addPolygon` and runs the game's two DSP plug-ins from the banks: the plug-in ABI
(`FMOD_DSP_DESCRIPTION`, `FMOD_DSP_STATE`, the parameter descriptions, the data-parameter types)
is the same in 2.03's headers as in 1.08's, and 2.03 accepts the game's plug-in SDK version 108.
The credit "Audio engine: FMOD Studio by Firelight Technologies Pty Ltd." is printed when the
sound starts.

Proofs (`re/scratch/taskP3/`):

| What | Against | Result |
|---|---|---|
| The `sound` area's 18 drives: the port's side on Linux (`tape_proof`, a new example: the page's `sound_proof` with native paths; `tapes.sh`) | the page's 2.03 HTML5 path in node (`page18.sh`: `tools/web_checks/task41/node_proof.mjs` on this branch's site) | **18 / 18 identical** call logs (`snd_cmp.py`, `l6s_cmp18.log`) |
| the same 18 | Windows' port side with FMOD 1.08 (`audio_oracle run --side port`, `desk18.sh`) | 11 identical; the other 7 differ only where FMOD 2.03 ends an event earlier than 1.08 (it answers stopping or stopped where 1.08 answers playing), the same 7 as the page in Task 41 (`docs/port/web_sound.md`) |
| The area's three end-to-end rows: Linux's `rustyac --replay ... --headless --audio-script <the game's log>` (`e2e_linux.sh`) | Assetto Corsa's own sound code (`audio_oracle run --side ac`, FMOD 1.08) | **3 / 3 identical** (`logdiff.py`), the game's own hashes |
| The mix (WAV, 3 drives, levels per second from each file's first sound, `levels.py`) | Windows' port, FMOD 1.08 | 0.36 to 0.76 dB RMS apart (Windows against itself: 0.10 to 0.13 dB; the page without the plug-ins was up to +8 dB on trackside) |
| A real-time drive in WSLg (`listen.sh`: `--headless --realtime --audio-log`) | | FMOD found one output device (WSLg's PulseAudio, `PULSE_SERVER`), the game held its socket, the mixer ran the 24 s at 1.6 % of a core. **Whether it is heard is Super's to check** (`listen.sh`, by ear) |

Two things the comparison leaves out, both FMOD's and not the mapping's: the setup's loop over
the sound devices (`snd_cmp.py` skips `getNumDrivers` / `getDriverInfo`: this PC has 5 under
Windows, WSL none without the WAV writer), and in its clock-less mode FMOD 2.03 mixes a few silent
blocks while each bank loads (about 1 s more silence before the drive's first sound than 1.08), so
WAV files are compared from their first sound. A headless run without `--audio-log` or
`--audio-wav` has no sound at all, as on Windows. Windows: the `sound` area's quick rows 4 / 4
identical (`l6s_quick_sound.log`); clippy `-D warnings --all-targets` clean on Linux, Windows and
both wasm targets, `audio_oracle`'s too.

**The licence** (`LICENSING.md`, done now as the prompt asks): an entry for FMOD Engine 2.03.15
for Linux and macOS (the release zips carry Firelight's unchanged `libfmod` / `libfmodstudio`,
under the FMOD EULA, not the GPL, loaded at run time through the C interface; the credit "Audio
engine: FMOD Studio by Firelight Technologies Pty Ltd." when the sound starts and in the zips'
notices), and the FMOD section says the Linux and macOS zips carry the libraries and that FMOD is
in no release's Corresponding Source. The section-7 permission itself needed no change (it names
the FMOD Engine, not the DLLs).

**The required-files list:** `web_pack --config <cars ["*"], tracks ["*"]> --required <folder>`
(`docs/flags.md`) probes every car and track as the pack does (`files_read_by_drive` +
`files_of_picture` + the sound banks per car and track, the shared files, AC's launcher's files)
and writes two manifests in the pack's format, paths and sizes only (no hashes, nothing copied):
`base.json` (Kunos' cars and tracks; DLCs are Kunos' too, so a player missing one sees that
DLC's cars or tracks named as missing, as section 5 planned) and `everything.json` (mods too).
On this install (about 5 minutes; `re/scratch/taskP3/required/`, `l6_required.log`):

| List | Cars | Tracks and layouts | Shared | AC's launcher | **In all** |
|---|---|---|---|---|---|
| base | 178 (15.4 GB) | 46 (22.7 GB; layouts share files) | 1.18 GB | 0.47 GB | **28.6 GB, 19,714 files** |
| everything | 189 (17.6 GB) | 64 (30.8 GB) | 1.18 GB | 0.54 GB | **34.3 GB, 21,372 files** |

Left out of both (28): content made for Custom Shaders Patch (CSP is Windows only) and two DLC
cars this install does not own. Against section 5's 38 GB for the whole folder: the rest is what no
drive reads (CSP, its content, unused files). Checks: `web_pack` clippy `-D warnings --all-targets`,
its 6 tests; the `browser` area (its flags check now lists `--required`).

**Split into the base game and the DLCs** (the lead chat's answer after session 1). How a car or
track is assigned: by **the DLC key in its models' headers**, the game's own rule. A kn5 of
version 6 and up carries a key after its version (0 for the base game); `KN5IO::load` refuses a
model whose key the player's Steam account does not own, and `SteamInit` maps each key to a DLC's
Steam app (`rustyac-game`'s `steam::DLC_KEYS`, 11 of them). The apps' names are Steam's own,
from its local app cache (`appcache/appinfo.vdf`, each app's record read by its id and length,
`re/scratch/taskP3/dlc_names.py`); Steam's install record (`appmanifest_244210.acf`) lists the
same 11 apps as installed depots. Kunos' `dlc_ui_car.json` / `dlc_ui_track.json` markers (what the
launcher shows a DLC badge for) agree with the keys on every marked car and track, but miss two
Kunos cars that carry a DLC key without a marker (`ks_lamborghini_countach_s1`,
`ks_lamborghini_gallardo_sl_s3`): the keys are the rule, the markers are not
(`re/scratch/taskP3/dlc_survey.py`). `web_pack --required` now writes `base.json` (Kunos' content with
key 0, the shared files, AC's launcher's), one `dlc_<app>.json` per DLC (its cars and tracks and
their own launcher pictures, no shared files: checked only when one of them is picked, "this car
needs the Dream Pack 1 DLC") and `everything.json` as before. A Kunos model with a key of no DLC
in the table stops the run. Mods that reuse a DLC's models carry its key too (two here:
`bdc_streetspec_ae86_v4`, Japanese Pack; `urd_darche_992_23`, Porsche Pack III): they stay in
`everything.json` only, and L7's check of a mod (whether it loads) is where the missing DLC shows.

| List (`re/scratch/taskP3/required/`, `l6d_required.log`) | Steam app | Cars | Tracks and layouts | **Size, files** | Steam's depot |
|---|---|---|---|---|---|
| `base.json`: the base game | 244210 | 86 | 36 | **17.6 GB, 11,016** (shared 1.18 GB, launcher 0.20 GB) | 27.5 GB |
| `dlc_347990.json`: Dream Pack 1 | 347990 | 10 | 4 (`ks_nordschleife`: four layouts) | 2.04 GB, 1,128 | 2.82 GB |
| `dlc_404430.json`: Dream Pack 2 | 404430 | 10 | 2 (`ks_barcelona`) | 1.23 GB, 1,046 | 1.70 GB |
| `dlc_423630.json`: Dream Pack 3 | 423630 | 9 | 2 (`ks_brands_hatch`) | 1.23 GB, 978 | 1.96 GB |
| `dlc_467980.json`: Japanese Pack | 467980 | 12 | 0 | 0.94 GB, 911 | 1.50 GB |
| `dlc_493190.json`: Red Pack | 493190 | 7 | 2 (`ks_red_bull_ring`) | 1.21 GB, 801 | 1.73 GB |
| `dlc_509350.json`: Tripl3 Pack | 509350 | 3 | 0 | 0.26 GB, 364 | 1.44 GB |
| `dlc_526060.json`: Porsche Pack I | 526060 | 7 | 0 | 0.67 GB, 710 | 1.19 GB |
| `dlc_540710.json`: Porsche Pack II | 540710 | 9 | 0 | 0.85 GB, 642 | 1.10 GB |
| `dlc_540711.json`: Porsche Pack III | 540711 | 8 | 0 | 0.87 GB, 592 | 1.20 GB |
| `dlc_619680.json`: Ready to Race Pack | 619680 | 10 | 0 | 0.95 GB, 875 | |
| `dlc_675590.json`: Ferrari Pack (Steam's name; the 70th Anniversary pack) | 675590 | 7 | 0 | 0.75 GB, 651 | |
| `everything.json`: DLCs and mods too | | 189 | 64 | **37.4 GB, 21,969** | |

The base game and the 11 DLCs add up to session 1's Kunos list (28.6 GB, 178 cars, 46 tracks and
layouts): nothing is lost or counted twice. Each list is smaller than its Steam depot (what no
drive reads: unused files, CSP content). `everything.json` grew since session 1 (34.3 GB, 21,372
files): the install's mod tracks changed in between, not the code (session 1's program gives the
same 37.4 GB on today's install). Checks: `web_pack` clippy `-D warnings --all-targets`, its 7 tests
(one new: a model's header names its DLC); `docs/flags.md` says what `--required` writes.

### 12.4 Resume here (P3b), plan changes, not done

- **P3b is done.** Next: L7 (the folder / zip chooser, which checks `base.json` and the DLC
  lists), then L8 (the hardware list below).
- Tools (`re/scratch/taskP3/`): `fakedev.py` + `l4run.sh` (L4, as root), `shm_win.ps1` /
  `shm_linux.sh` (L5; never while Assetto Corsa runs), `all.toml` + `web_pack --required` (L6's
  list), `tapes.sh` / `page18.sh` / `desk18.sh` / `snd_cmp.py`, `e2e_linux.sh`, `levels.py`,
  `listen.sh`, `smoke.sh` (L6's sound), `wsl.sh` / `wasm.sh` (builds, wasm clippy; run `wsl` with
  `MSYS_NO_PATHCONV=1` from Git Bash).
- **Plan changes:** **L7**'s folder / zip check reads these manifests: ship `base.json` and the 11
  `dlc_<app>.json` (made from a full Kunos install: paths and sizes of Kunos' files, none of their
  content) with the native zips; missing base files: "this folder is not a complete Assetto
  Corsa"; a picked car or track in a DLC list whose files are missing: "this car needs the <name>
  DLC" (the names: the table in 12.3, to carry into L7's code). Mods are checked when chosen
  (whether they load), since no shipped list can know them.
  **L8** gains the hardware list of 12.1 (a real wheel's force direction and DirectInput's
  rounding on Windows against Linux, real pedal ranges, hot-plugging, non-Xbox pads) and a
  real Linux PC's PulseAudio / PipeWire listen. Nothing else in L7 / L8 changes.
- **Not done:** the hardware checks (L8); hot-plugging a wheel during a drive
  on Linux (found at the next start only); a Windows run of the third page drive (Assetto Corsa
  was running).
- **Windows unchanged:** every Windows line of the changed files is as it was (`cfg` attributes
  and `cfg!` added, the Windows strings the same); quick proofs of every area the changed files
  touch: fresh 63, csp 71, sound 4, replays 6, ai_page 10, launcher 1, dumps 6 IDENTICAL, browser
  18 IDENTICAL on its rerun (its one difference in the first run was the flags check: `docs/flags.md`
  now lists `--required`) (`quick_l5.log`,
  `quick_l6_browser.log`).

## 13. Task P3c: the launcher on wry, the game-files chooser, packaging (L7 to L8)

### 13.1 L7 part 1: the portable pieces (done, `rustyac-content`)

For every platform (the web build and the Windows launcher wire them in with Task W5):

- **`imports`**: what a chosen folder or zip holds: the whole game (`content` with cars or
  tracks, and `system`), cars and / or tracks (`content/cars|tracks`, `cars|tracks`), or one car
  (`data.acd` or `data/`, and `ui/ui_car.json`) or one track (`models*.ini` or a `.kn5`, and
  `ui/`); a folder or zip with only one folder in it is looked into (a zip of a folder).
  **`Layers`** lays the extra imports' cars and tracks over the base's `content/cars` and
  `content/tracks` (the base wins, then the first extra), reads every file in place, and is a
  `vfs::Vfs`, so the game's readers need nothing new; a zip base stands at `<zip>/<folder>`.
  `Layers::open` gives the player's messages ("Assetto Corsa's install folder wasn't found:
  choose the folder with the game files", "… holds cars or tracks only …", "… holds no cars or
  tracks …"). **`Remembered`**: the choice in `game_files.txt` (`root=`, `extra=`) under
  `$XDG_CONFIG_HOME/rustyAC` (else `~/.config`), `~/Library/Application Support` on macOS,
  `%APPDATA%` on Windows.
- **`zip`**: only the central directory is read when the zip is opened; each file is read out of
  the archive when asked for and checked against its CRC; zip64; stored and deflate (anything
  else, or an encrypted entry: "unzip it and choose the folder"). `miniz_oxide` and `crc32fast`,
  already in the tree, pure Rust.
- **`required`**: `base.json` and the `dlc_<app>.json` lists (12.3) read back; `check_full`
  ("this folder is not a complete Assetto Corsa: N files of the base game are missing:
  content/cars/abarth500 (2 files), system/data (1 file), …"), `check_pick` ("this car needs the
  Japanese Pack DLC"), `loads` for anything in no list (the car's data and the models its
  `lods.ini` names, or the layout's models, opened as kn5; a model carrying a DLC's key needs that
  DLC's files). The DLC table (key, app, name) moved here from `web_pack`, which uses it.
- Tests on made-up folder trees and zips (built in the test, zip64 too): 9 kinds of folder and
  their zips, layered reads, the messages, a mod's DLC key; and on this PC the real lists against
  the real install (11,016 base files there, a base car loads). Linux, Windows and both wasm
  targets clippy clean.

### 13.2 L7 part 2: the launcher on Linux (built, not yet proved)

- `crates/rustyac-launcher/src/shell_gtk.rs` (the module `shell` on Linux) and `sys_unix.rs` (the
  bridge's host from GDK / GLib): a GTK window with WebKitGTK 4.1 through `wry` 0.58 (no tao, no
  x11 feature), the same `Bridge`, theme, script steps and log as the WebView2 shell. The pieces
  that do not depend on the browser moved verbatim from `shell.rs` to `page.rs` (theme, events,
  steps, the wizard script, the controls page, Drive's command line, which now spells its paths
  the system's way through `os_path`: unchanged on Windows).
- **The bridge's synchronous requests:** WebKitGTK's custom schemes (and WKWebView's) give no
  synchronous loads, and the theme's `$.ACCall` is `async: false`: the first synchronous request
  failed inside WebKit ("loadResourceSynchronously … internal error") and the page hung. The
  page's XMLHttpRequests now go to a small server of the launcher's on 127.0.0.1 (a free port, a
  random token in every path, one write per answer: two writes waited on the delayed ACK, 50 ms a
  request), which hands each to the main loop; `http://ac/` POSTs (CEF's
  `OnBeforeResourceLoad`) are rewritten in the same hook. URLs are spelled as Chromium spells them
  (host lower case, an empty path `/`), so the log matches Windows'. A scripted run's proxy
  (127.0.0.1:9) leaves 127.0.0.1 alone.
- **The chooser** (`choose_game_files`, before the theme unless `AC_ROOT`, `--ac-root`, a script
  or a transcript names the folder; `--choose-files` brings it back): the remembered choice when
  it still opens and checks out, else rustyAC's own page: "Assetto Corsa found in <folder>. N cars
  and M tracks." or the message of 13.1, **Change… (a folder)** / **Change… (a zip)**, **Extra cars
  and tracks** (Add a folder… / Add a zip… / Remove), **Continue** only when the choice checks out
  against `required/base.json` beside the program. GTK's native file chooser (portals where the
  desktop has them).
- **Drive** checks the race's track and every distinct car (`check_pick`, else `loads`) and shows
  the message instead of starting the game. The game gets `AC_ROOT`, or `RUSTYAC_GAME_FILES` (the
  remembered file) when a zip or extra roots are in the choice: the game (off Windows) then mounts
  the same layers at its start (`Remembered::mount_file`). FMOD's `loadBankFile` takes a bank the
  disk does not have from the mounted files (`LoadBankMemory`, `fmod/native.rs`).
- New crates (Linux only; every Windows entry of the lock file keeps its version, only second
  versions were added): `wry`, `gtk`, `webkit2gtk` and what they pull, in `LICENSING.md`
  (61 names, and WebKitGTK's own entry). `docs/flags.md`: `--choose-files`, `RUSTYAC_GAME_FILES`.

**Measured, and where it stops.** Under Xvfb in WSL (`re/scratch/taskP3/l7_run.sh <name>`: a
click script of `tools/proofs/launcher/`, X11, WebKit's caches in the run's folder) the theme
loads and 2,600 to 2,700 bridge requests are answered, then the page stalls. Two causes found:

1. **WSL's `/mnt/c` is 33 times slower than Windows for the bridge's listing work**:
   `listcars` + `listtracks` + `listspecialevents` take **337 s** on Linux through 9P against
   **10.4 s** on Windows (`l7_time.sh`, the same three requests through `--transcript`). The main
   loop is busy that long.
2. While it is busy, the page's synchronous requests fail in WebKit with `NetworkError: A network
   error occurred` without reaching the server (`uimessage`, `carcount`, `uiready`, `getresults`…;
   a lone synchronous request on a free main loop is answered with status 200). Not yet known: a
   WebKit / libsoup limit (connections per host, a time limit) or something of the server's.

On a native Linux disk the listing should take about what Windows takes; this PC cannot show it
(the repo and AC's files are on `/mnt/c`, and files stay in the repo, rule 6).

### 13.3 Resume here (P3c)

- **Done and committed:** 13.1 (part 1) and 13.2 (part 2, built and clippy clean everywhere).
  Windows: the launcher area's quick proof **1 / 1 IDENTICAL** (`l7_quick_launcher.log`); every other area skipped
  because no Windows code path of theirs changed (the content modules are new, the `desktop.rs`
  and `fmod/native.rs` changes are `cfg(not(windows))`, the lock file keeps every Windows version).
- **Next (L7, about one session):**
  1. Find the `NetworkError`: hold the main loop on purpose (a test-only delay) for 5 s and 70 s
     before answering, to tell a time limit from a connection limit; if the main loop is the
     cause, answer the bridge on a thread of its own (the `Bridge` behind a mutex, events and log
     to the main loop) so a long listing never stalls the page's other requests.
  2. The proof: record **fresh Windows request lists** into scratch (not `re/gold/launcher`, which
     was recorded before this install's mods changed: 62 tracks and 207 cars there, 71 and 210
     now) with `launcher_record.sh`'s steps, run the same scripts on Linux (`l7_run.sh`), compare
     the lists without jQuery's `_=<time>`, and the `race.ini` each writes. Then the transcripts:
     every `re/gold/launcher` list through Linux's `--transcript --fixed-host` against Windows'.
  3. `ipc` messages (the page's console through `REPORT_JS`) do not reach the log on Linux; the
     console goes to the launcher's own output (`enable-write-console-messages-to-stdout`) for
     now. Find why.
  4. A zip end to end: the game's readers that use `std::fs` directly (`render/gui.rs`,
     `gui_apps.rs`, `conditions.rs`, … about 70 calls) and whether any of them reads AC's own
     folder.
  5. A real Linux disk (the tester, L8) settles cause 1 of 13.2.
- **Then L8:** `release.yml`'s Linux row (the two programs, FMOD's Linux libraries and their EULA
  credit, `required/`, `THIRD_PARTY_NOTICES.txt` made again with `tools/notices.py` since the lock
  file changed), the tar.gz, what a player needs (Vulkan drivers, `libwebkit2gtk-4.1`), and the
  tester's kit (CachyOS: `webkit2gtk-4.1`, `vulkan-icd-loader` and the GPU's Vulkan driver,
  `sdl3` if it is not static; one script that runs the checklist and packs the logs).
- **Open question for Super:** letter case. On a real Linux disk a Proton install keeps Windows'
  file names, but the game and the theme ask for files in whatever case their code spells them;
  Windows and `/mnt/c` do not care, ext4 does. The tester's run will show whether a lookup that
  ignores case is needed (the zip reader already ignores it).
