# Proof farm (Task F1 spike, Task F2)

Can a Windows 10 virtual machine on the home server run the proof areas bit-exact, and how
fast? The VM has 12 virtual CPUs, 32 GB of memory and no GPU. Its host processor is an Intel
Xeon E5620 (Westmere, 2010), which has **no AVX and no FMA3**. The PC is an i9-9980HK, which
has both. Task F1 is a spike: it measures and judges, but it builds no scheduler (that is F2).

## Resume here

- Done: step 1 (part 1), step 2 (part 2: the physics area on the VM, bit-exact; the maths
  checks).
- Done: step 3 (part 2b, on paper) and step 4 (part 2c: GO with conditions). F1 is finished.
- **F2**: part 1 done (paths from the environment, the maths path in the cache key, a correctly
  rounded fma; its quick run finished: every area IDENTICAL except csp's two `refused` cases, which
  fail on master too). Part 2 done: `RUSTYAC_PREBUILT=1` with `prebuild.sh`, `farm_sync.sh`; the
  physics part of the copy is on the VM, the rest of the one-time copy (~85 GB) waits: SMB gave
  5.3 MB/s over Wi-Fi. Part 3 step 1 done: the full physics area is 121/121 IDENTICAL on the VM
  with `RUSTYAC_MATH=pure` (94/121 on the DLL path).
- Part 3 step 2 done: session 0 fails render, gui and the launcher (no desktop: no swap chain,
  no screen modes); a scheduled task with `/IT` in the logged-in session runs all three
  IDENTICAL. `farm_sync.sh` has the `ac` and `repo` parts for the rest of the copy (Super runs it).
- Part 3 step 3 done: the copy finished (~5 MB/s); every area but shaders, browser and ai_page is
  IDENTICAL on the VM in its quick run (yebis_sweep on a 7-row sample); shaders stays on the PC
  (WARP's and SwiftShader's pixels change with the processor). WARP areas run 1.5-2.2 × the PC's time.
- Part 4 done: `run_all.sh --farm` (`farm.sh`, `farm_run.sh`, `farm_task.cmd`, lane shards, the
  fallback to the PC, `proof_tui`'s VM lanes, `old_exe.sh` for new tags). Part 5: `--quick --farm`
  286/286 IDENTICAL in 0h45m (PC alone 0h59m).
- **F3** (below): the settings moved to `%USERPROFILE%\.rustyac\farm.env`; `--farm` stops at
  once when the farm isn't set up or doesn't answer.
- **F3 done**: the full run's 4 non-identical cases explained, fixed and IDENTICAL again; the copy
  leaves out the PC's kept game results; `--quick --farm` 288/288 IDENTICAL, all farm areas on the VM.
- The first real `--full --farm` (2026-11-23, main folder): every area identical, but 9h14m: the
  PC's lanes were done after about 1.5 h, then it waited about 8 h on the VM's fixed lanes.
- **F4** (below): one shared queue of shards that the PC's free lanes and the VM's eight lanes both
  take from; csp runs on the PC only; `farm_lanes_full` / `FARM_LANES` are gone.
- **Resume here: F4 is done.** Next: Super's next `--full --farm` (estimate in F4: likely about 4h with
  the server as busy as on 2026-11-23, about 3h with it idle). Optional: browser and ai_page in two PC lanes.

## Part 1: what the processor changes (from the code)

### The msvcr120 switch

`MSVCR120.dll` (12.0.21005.1) holds two bodies for `sinf`, `cosf`, `tanf`, `expf`, `logf`,
`powf` and `sin` (double). Each one starts with `cmp [__use_fma3_lib], 0`. The runtime sets
that flag once, at load time, from the processor's FMA bit alone (`__fma3_lib_init`, 0x180081b3c:
CPUID leaf 1, ECX bit 12; no OSXSAVE or XGETBV check in this build; F2 read it).
`acs.exe` never calls `_set_FMA3_enable`, so the flag follows the processor.

- **The PC** (i9-9980HK): flag = 1, so the FMA3 bodies run.
- **The server's VM** (E5620): flag = 0, so the SSE2 bodies run.

The two bodies do not agree everywhere. `math_proof fma3-off` (docs/port/web.md, the
"pre-2013 CPU" table) counts these differences:

| function | inputs whose result differs |
|---|---|
| `sinf` | 44,232 of 2^32 |
| `cosf` | 18,018 of 2^32 |
| `expf` | 1 of 2^32 |
| `powf` | 7,020 of 400 M pairs |
| `sin` (double) | 954,879 of 100 M in ±1e6 |
| `tanf`, `asinf`, `acosf`, `atanf`, `atan2f` | none |

So AC itself gives different bits on the server than on the PC whenever a drive reaches one
of those inputs. A long physics drive does reach them: sin and cos of wheel and steering
angles, and powf in the tyre and aero code.

### Which path the port takes

`crates/rustyac-math` routes these functions in one of three ways:

- **Default on the desktop**: load `MSVCR120.dll` at run time and call it. The port then
  takes **the same path as the processor**, exactly as the game does.
- **`RUSTYAC_MATH=pure`**: the Rust ports of the FMA3 bodies (`pure.rs`), which give the
  same bits on every processor. Their three `vfmadd` steps are `f64::mul_add`, a single
  correctly-rounded operation. On a processor without FMA3, Rust's `mul_add` becomes a
  software fma, which is correct but slower. Whether its result matches the PC's is checked
  on the VM in step 2 (`math_proof digest`).
- **wasm**: always pure.

No proof script sets `RUSTYAC_MATH`, so every area runs the default DLL path.

The in-process oracles (`car_oracle`, `csp_oracle`, `render_oracle`, `audio_oracle`) bind
`msvcr120.dll` from the system search path (`tools/car_oracle/src/acs.rs:31`, `:402`). The
game's code and the port then share one DLL in one process, so they both take the same path
on any machine. **A live comparison, game against port in the same run, is a fair proof on
the server.** It proves the port matches AC on a Westmere, which is a stronger claim than
the PC alone can make.

One more processor-dependent instruction is `rcpps` (`crates/rustyac-ode/src/matrix.rs`). On
x86 the hardware instruction runs; wasm uses Intel's 2048-entry table (`rcp_table.rs`). The
table is proven equal to the i9's hardware only. Westmere's `rcpps` is not proven equal, and
step 2 runs `math_proof rcpps` there. If they differ, a kept result containing an LCP solve
would differ for that reason as well.

### Build flags

There is no `.cargo/config.toml`, and no `target-cpu` or `target-feature` in any toml or
script. Every exe is built for baseline x86-64 (SSE2), so the PC's exes run on the E5620
without a rebuild. None of the Rust code itself picks a path by processor. The one
exception is a test: `crates/rustyac-math/tests/pure.rs:104` prints NOT TESTED on a
processor without FMA3.

### Kept results made on the PC

Some areas compare against results the game made earlier on the PC, not against a game run
in the same process. Those results carry the FMA3 path, while the server's port takes SSE2:

| area | what it compares against | made on the PC? |
|---|---|---|
| physics | the recordings in `oracle/car*` and `oracle/**/*.mcrec` | yes, all of them |
| fresh | kept recordings `re/gold/fresh/rec` (cache), or new ones when the key is new | kept: yes |
| replays, dumps | `oracle/game` drives, `re/gold/docs`, `re/gold/dumps` | yes |
| browser | the desktop's replay of `oracle/game` against wasm (always pure, so FMA3 bits) | the drives, yes |
| net | `re/gold/net` goldens, four ways (two of them wasm) | yes |
| ai_page | `re/gold/ai_page` | yes |
| launcher | `re/gold/launcher` request lists | yes (no maths: requests and answers) |
| shaders | `re/gold/shaders` tables | yes (no maths on the CPU path: shader bytes, GPU runs) |
| csp, render, gui, python, sound | game and port live, same process | no |

The cache key (`tools/proofs/lib.sh`, `salt_of` and `install_key`) is made from the tool's
git tree, its diff and the install. **It has no processor in it.** A PC-made recording is
therefore reused on the server, and a server-made one would poison the PC's cache if the
two caches were ever shared.

### Conclusion per kind of area (to be measured in steps 2 and 3)

- **Live, same process** (csp, render, gui, python, sound, fresh's new recordings): fair on
  the server as they are. Both sides take the SSE2 path. Render and gui already run on WARP
  (software), which has no GPU to miss. Sound uses FMOD's mixer, which is not tied to the
  processor's path (to be checked).
- **Replay against PC-kept recordings** (physics, fresh's kept recordings, replays, dumps,
  browser, ai_page, net): expected DIFFERENT on the DLL path wherever a drive meets the
  inputs above. The fix to try is `RUSTYAC_MATH=pure` on the server. The port then follows
  the FMA3 bits the recordings were made with, through the software `mul_add`. That holds
  only if `math_proof digest` matches the PC on the server and Westmere's `rcpps` equals
  the table. Otherwise the kept results would have to be made on the server (a second set,
  keyed by processor).
- **No processor maths** (launcher, shaders' table steps): fair on the server.

Step 2 measured two of these guesses wrong: the quick physics area is IDENTICAL on the SSE2 path
against the PC's recordings, and `RUSTYAC_MATH=pure` is no fix on the server (the UCRT's software
fma; part 2's maths section).

## Part 2: the physics area on the VM (step 2)

### What went to the VM

Everything sits in `C:\rustyac-farm\` on the VM. The script is `re/scratch/taskF1/copy.sh` (taskF1 is archived on the server: `taskF1.tar`, see docs/disk.md)
(tar over ssh); 9 min for 1.1 GB and 10,104 files.

| what | size |
|---|---|
| `car_oracle.exe`, `chassis_compare.exe` (release, `--locked`, built on the PC) | 4.9 MB, 2.1 MB |
| the PC's `msvcr120.dll` (12.0.21005.1) and `vcruntime140.dll` beside each exe | 0.9 MB, 0.2 MB |
| `tools/proofs/` | 0.2 MB |
| `cardata/` | 11 MB |
| `oracle/car_wc/` and the five recordings the quick run names | 186 MB + 69 MB |
| an empty `oracle/car/` (physics.sh only checks that it exists) | 0 |
| AC: `content/tracks/spa`, `content/tracks/magione`, `system/data/surfaces.ini` | 516 MB, 308 MB, 1 KB |
| AC: `content/cars/*/collider.kn5` (all 207) | 1.5 MB |
| a `cargo` stand-in that does nothing (run_all.sh builds the shared programs first) | 1 line |

No kept `re/gold/` part and no fake root (`re/scratch/task46c/fakeac`) is needed: the physics
area compares against `oracle/` only and never loads the game.

**Which system DLLs.** `car_oracle` and `chassis_compare` import `KERNEL32`, `ADVAPI32`,
`ntdll`, `bcryptprimitives`, `VCRUNTIME140` and the UCRT (`api-ms-win-crt-*`). They load
`msvcr120.dll` at run time (`LoadLibraryA`, `crates/rustyac-math/src/lib.rs`). The physics area
never loads `acs.exe` (`multi-check` and `chassis_compare` are port only).

| DLL | PC | VM |
|---|---|---|
| msvcr120.dll | 12.0.21005.1 | 12.0.40664.0 (another build) |
| msvcp120.dll | 12.0.21005.1 | 12.0.40664.0 |
| vcruntime140.dll | 14.51.36247.0 | missing |
| ucrtbase.dll | 10.0.19041.3636 | 10.0.19041.3636 |
| dbghelp.dll | 10.0.19041.5848 | 10.0.19041.3636 |
| kernel32.dll | 10.0.19041.5915 | 10.0.19041.3636 |

So the farm copy carries the PC's `msvcr120.dll` and `vcruntime140.dll` beside each exe (the
exe's folder comes first in the DLL search). Then the processor is the only difference.

**Changes in the farm copy (not in the repo).**

- Environment only: `HOME` and `USERPROFILE` point into `C:\rustyac-farm\home` (the proof lock
  `~/.rustyac` stays in the farm), and `PATH` starts with the `cargo` stand-in.
- The exes have the PC's repo path and AC path baked in (`CARGO_MANIFEST_DIR` in `repo_root()`,
  plus panic messages): 153 places in `car_oracle.exe`, 91 in `chassis_compare.exe`. The farm copies
  are patched to same-length farm paths (`re/scratch/taskF1/patch_paths.py`). F2 needs a real
  override (an environment variable or the exe's own folder), not a byte patch.
- No script changed. The first VM run had 5 ERRORs, "no collider.kn5 for bmw_m3_e30": the
  recordings name cars whose `collider.kn5` is read from AC's folder. Copying the 207 collider
  files fixed it. That was the harness, not the processor.

### Verdicts: VM against the PC

`bash tools/proofs/run_all.sh --quick physics`, same commit (5696c8bf), same exes:

| | cases | IDENTICAL | DIFFERENT | ERROR | wall clock |
|---|---|---|---|---|---|
| PC | 9 | 9 | 0 | 0 | 1m35s |
| VM | 9 | 9 | 0 | 0 | 1m03s |

Byte compare of the two result folders: `cases.log`, `race_out.json`, `referee.txt`, `laps.ini`
and `oracle/car_wc/results.md` are the same bytes. `result.txt` and the area's `*.txt` differ
only in timings ("1.2 s" against "1.1 s") and in one output path line. Every hash is the same
(e.g. `mc_side_by_side` 42792595eb120eed, `rc_race` 16d26c43d9d21d0a).

The port took MSVCR120's SSE2 path on the VM (checked below), and the recordings were made by
the game on the PC's FMA3 path. They still agree, because the inputs where the two paths differ
are far from what these drives compute (the first `sinf` ones are near 8.4e6 radians; see the
maths section). That holds for these 9 cases. It is not a proof for all 113 of `--full`, whose
recordings (about 12 GB of `oracle/`) were not copied in the spike.

### Time per case and lanes

`times.tsv`, seconds (the PC idle, nothing else running; the car_wc folder's time is folded
into the first case):

| case | PC | VM |
|---|---|---|
| car_wc (4) + `mc_side_by_side` | 35 | 15 |
| `ai_solo` | 10 | 8 |
| `rc_race` | 10 | 6 |
| `rc_jump` | 32 | 25 |
| `pit_mouse` | 7 | 6 |
| area wall clock | 95 | 63 |

**VM / PC = 0.66** on this area: the VM is faster. The PC is a laptop (i9-9980HK, Balanced power
plan) and ran at 1.4 GHz during the measurement. Its own number moves with heat and the power
plan, so the ratio is this PC's, today.

Lanes: n copies of the farm repo (`re/scratch/taskF1/stage/vm_lanes.sh`, archived), n quick physics runs
at once (`PROOF_LOCKED=1`), every lane 9 IDENTICAL:

| lanes | wall clock | throughput (× one lane) |
|---|---|---|
| 1 | 64 s | 1.0 |
| 2 | 67 s | 1.9 |
| 4 | 66 s | 3.9 |
| 6 | 70 s | 5.5 |
| 8 | 78 s | 6.6 |
| 12 | 102 s | 7.5 |

Scaling is nearly flat up to 6 lanes and bends after 8. The host has 2 × 4 cores with two
hyperthreads each, and the VM's 12 vCPUs share them. **8 lanes is the useful limit**, and 12
still gains a little.

### The maths on the Westmere (`math_proof` on the VM)

`re/scratch/taskF1/stage/vm_math.sh` (archived); results in `re/scratch/taskF1/vm_math.txt`.

- **`rcpps`**: Westmere's hardware against `rustyac_ode`'s table copy of Intel's, every 32-bit
  pattern: **identical**. The LCP solver gives the same bits on the server as on the PC.
- **Which DLL path**: `math_proof one sinf cosf expf` with the PC's `msvcr120.dll` reports "the
  DLL's FMA3 paths are off" and finds sinf 44,232, cosf 18,018, expf 1 differences against the
  FMA3 bodies: the same counts as the PC's `fma3-off` run. So on the server both the game and the
  port take the SSE2 path. The first differing `sinf` inputs are near 8.4e6 (`4b000300`), far
  beyond the angles a car computes, which is why the physics drives above still agree.
  The VM's own `msvcr120.dll` (12.0.40664.0) gives the same three counts, so the DLL's build
  does not matter here; the processor does.
- **`RUSTYAC_MATH=pure` is not a fix on the server.** `math_proof digest` (pure maths, a fixed
  input set) matches the PC for 9 of 11 functions but **differs for `logf` and `sin`**
  (`8347f9697e788f84` against `02ec5c197611c13a`, `b446b737d725d39f` against
  `4c16ad2456cf6445`). The cause: `pure.rs` uses `f64::mul_add`, which Rust turns into a call
  to the UCRT's `fma` on a processor without FMA3, and that `fma` is not correctly rounded there.
  `re/scratch/taskF1/stage/fma_check.py` (ctypes, against the exact `Fraction` result,
  300,000 triples): PC 0 wrong, **VM 10,673 wrong** (same `ucrtbase.dll` 10.0.19041.3636; on
  the PC it uses the hardware instruction). A software fma that is correct (the `libm` crate,
  already in `Cargo.lock`, or what wasm uses) would fix it. That is a port change, for F2 or later.
  The quick physics area with `RUSTYAC_MATH=pure` on the VM is still 9 of 9 IDENTICAL (1m12s): those
  drives do not reach the wrong fma results either.

## Part 2b: every area on paper (step 3)

What each area needs, from its script in `tools/proofs/`. "Kept from the PC" means it compares
against results made earlier on the PC (part 1). Every area except physics runs `cargo build` /
`cargo run` / `cargo test` itself (its own examples and tests), so on the server each of those needs
either a Rust toolchain there or prebuilt programs. That counts as one change for every row.

| area | needs | browser | fixed ports | kept from the PC | where |
|---|---|---|---|---|---|
| physics | CPU only (port only) | no | no | `oracle/` recordings | **server** (measured) |
| dumps | CPU; builds the last release from a git worktree | no | no | `re/gold/dumps` | server after change: ship the old exe, or keep only the kept dumps |
| fresh | CPU; game code in-process (car_oracle), AC cars and tracks | no | no | `re/gold/fresh/rec` (16.8 GB) | server after change: copy the AC content and the kept recordings, a processor in the cache key |
| replays | CPU; game code in-process, the user's replays | no | no | `re/gold/docs` | server after change: copy, and the user's own replay files leave the PC |
| sound | CPU; FMOD and the game's sound code in-process | no | no | no (live) | server after change: copy; FMOD needs no audio device in its NRT mode (unproven on the VM) |
| csp | CPU; CSP 0.3.2 in AC's folder, game in-process | no | no | no (live) | server after change: copy the CSP install |
| render, gui, python, yebis_sweep | WARP (software D3D11), game in-process | no | no | no (live) | server after change: copy; ssh runs in session 0 with no desktop, so window and swap-chain creation must be tried first |
| shaders | WARP D3D11 and D3D12, SwiftShader Vulkan | no | no | `re/gold/shaders`, tables equal to Task 38b | server after change: WARP's own code may pick SSE or AVX by processor, so the pixel tables must be proven equal there |
| launcher | .NET SDK and NuGet (restore from the internet), WebView2 | WebView2 | no | `re/gold/launcher` | server after change: install the .NET SDK and WebView2, session 0 again |
| net | wasmtime, the wasm sysroot, the desktop way on MSVCR120 | no | no | `re/gold/net` (59 MB) | server after change: the desktop way meets the kept goldens on the SSE2 path |
| browser | Chrome, Node, wasmtime, the web pack (2 GB) | Chrome | 8139, 8140 | `oracle/game` drives | **PC only** for now: the biggest install, and its ports allow one run a machine |
| ai_page | Chrome, Node, the web pack | Chrome | 8137, 9338 | `re/gold/ai_page` | **PC only** for now, as browser |

Sizes on the PC to copy for the full set: AC `content/cars` 51.7 GB, `content/tracks` 12.8 GB,
`re/gold` 19 GB, `oracle/` 15 GB. At the spike's copy speed (1.1 GB in 9 min) that is about 13 h,
once; later runs only copy what changed (rsync).

### `--full` estimate

The PC's last measured full run (P1, docs/port/proofs.md, four lanes): 4 h 26 min for 1,678
cases, area times physics 85 min, fresh 205, gui 111, dumps 82, browser 84, render 75, replays 50,
net 46, ai_page 31, shaders 6, launcher 5, sound 4, python 0.5. The YEBIS sweep came after (about
1 h 30 min alone), and a full run is now about 6.5 h on the PC.

- On the PC: browser and ai_page, about 2 h (one lane, as today).
- On the server: the other areas, about 760 min of area time on the PC × 0.66 (the measured
  ratio) = about 500 min of server work; with 8 lanes at 6.6 × that is **about 75 min**, and with
  12 lanes at 7.5 × about 67 min.
- Together, the two run at once: **about 2 h** (the PC's browser lane is the longest), against
  about 6.5 h today.

The 0.66 is measured on CPU-bound port code only. WARP areas (render, gui, yebis_sweep, shaders)
spend their time in WARP's own code, which may run slower on a processor without AVX; nobody
measured that, and the server half could be up to twice as long (about 2.5 h), which would make
the server the longer half.

## Part 2c: verdict (step 4)

**GO with conditions.**

The numbers behind it:

- The quick physics area on the VM: 9 of 9 IDENTICAL, the same bytes as the PC, 0.66 of the
  PC's time. Lanes scale to about 8 (6.6 ×) and 12 still gains (7.5 ×).
- The server's processor changes the DLL path (SSE2, not FMA3) but not `rcpps`. A live
  compare, game and port in one process, is fair there. A compare against a PC-kept result is
  fair wherever the drive never meets the inputs where the paths differ. That held for every
  quick physics case, and it is still unproven for the rest.
- `--full` would take about 2 h instead of about 6.5 h (server ~75 min with 8 lanes beside the
  PC's ~2 h browser lane). The WARP areas' speed on the server is unmeasured and could double the
  server half.

The conditions, in the order F2 should meet them:

1. **Paths without a byte patch**: the oracles read the repo and AC roots from an environment
   variable (lib.sh already has `AC_ROOT`), not from `CARGO_MANIFEST_DIR` baked into the exe.
2. **A processor in the cache key** (`salt_of` / `install_key`: FMA3 yes or no), so results made
   on the server never stand in for the PC's or the other way round.
3. **The whole physics area on the server first** (113 cases, ~12 GB of `oracle/`). It is the
   cheapest broad test of "PC-kept results hold on the SSE2 path", before fresh, replays, dumps
   and net move.
4. **Prebuilt programs**: run_all.sh and most areas call cargo. Either a toolchain on the server
   or a mode that takes the PC's built exes (the stand-in proved the shared build can be
   skipped; `cargo run` / `cargo test` steps in the areas still need it).
5. **Session 0**: sshd's commands run without a desktop. One render, one gui compare and the
   launcher (WebView2) must be tried there before those areas move.
6. **No `RUSTYAC_MATH=pure` on the server** until `pure.rs` uses a correctly rounded software fma
   (the UCRT's is wrong on 3.6 % of the checked triples on the Westmere).
7. browser and ai_page stay on the PC (Chrome, Node, fixed ports).
8. A one-time copy of about 100 GB (AC content, `re/gold`, `oracle/`), about 13 h at the spike's
   speed, then rsync of what changed.

**F2's size: about 5 sessions.** (1) paths and the processor key; (2) prebuilt programs and the
copy/rsync script; (3) the full physics area and session-0 trials of render, gui, launcher; (4)
the PC/server split in run_all.sh and the results coming back; (5) one full run on both, fixes.

`C:\rustyac-farm` stays on the VM: 4.3 GB (AC parts 826 MB, the repo copy 275 MB, 12 lane copies 3.3 GB; delete `lanes\` first when space is needed).

## F2 part 1: paths, the maths path in the cache key, the fma

### Paths from the environment

Every oracle reads the repository from `RUSTYAC_REPO` and the game from `AC_ROOT` when they are
set, and falls back to what it did before (the folder it was built in, Steam's usual place):
`repo_root()` of `car_oracle`, `chassis_compare`, `audio_oracle`, `ode_oracle`, `render_oracle`
(canonicalized either way, as before); the default `--acs` of `car_oracle`, `audio_oracle`,
`ode_oracle`, `tyre_oracle`, `sctm_oracle` (`<AC_ROOT>/acs.exe`); `car_oracle`'s crash-report
function table; `render_oracle`'s Python test apps and the two track files of `gui_seq_apps`
(`rustyac_content::install::ac_root()`, which reads `AC_ROOT`); `acd_check`'s default cardata.
`render_oracle` and `csp_oracle` already took the game from `AC_ROOT` / the working folder, and
`rustyac.exe` already finds `cardata/` beside the exe and in the working folder first.

lib.sh does **not** set `RUSTYAC_REPO` (on the PC nothing changes); the farm sets it, in the
Windows form with backslashes that the built-in path had (`C:\rustyac-farm\...`), for each lane's
copy. The panic messages still carry the build's paths; they are text, not read.

### The maths path in the cache key

`install_key` (and so `salt_of`, every kept result's key, and `dumps.sh`'s key) now ends with
`maths SSE2` when MSVCR120.dll takes its SSE2 bodies, and with nothing when it takes the FMA3 ones,
so the PC's keys are the same bytes as before. lib.sh asks the DLL itself, once when an area
starts: `_set_FMA3_enable(1)` returns 1 only when `__fma3_is_available` is set, which is what
`__fma3_lib_init` reads from CPUID (python ctypes; an area stops when it cannot tell). The PC says
FMA3, the VM SSE2.

### `pure.rs`'s fma

`pure::fma` / `pure::fma32` are now the `libm` crate's `fma` / `fmaf` (0.2.16, already in the lock
files; MIT). On x86-64 it runs `vfmadd` when the CPU has FMA (or FMA4) and musl's exact software
algorithm otherwise; both are correctly rounded, so they give the same bits. `f64::mul_add` called
the UCRT's `fma` on a CPU without FMA3, which is wrong on the Westmere (F1).

`math_proof fma 64|32 <in> <out>` runs `pure::fma` / `fma32` on a file of triples;
`tools/math_proof/fma_check.py` checks them against the exact a*b+c (fractions, rounded to the
width with ties to even, subnormals and overflow included): F1's 300,000 triples (random, and c
near -a*b), the same shapes for floats, and the 5,832 triples of 18 special values (signed zeros,
the smallest subnormal, the largest subnormal, the smallest normal, one, one plus an ulp, the
largest finite, infinity, NaN). Results in `re/scratch/taskF2/`:

| | maths path | fma64 wrong | fma32 wrong | `math_proof digest` (pure) |
|---|---|---|---|---|
| PC | FMA3 (libm: the instruction) | 0 of 305,832 | 0 of 305,832 | same as F1's PC digest |
| VM | SSE2 (libm: software) | 0 of 305,832 | 0 of 305,832 | **same as the PC, all 11 functions** |

(F1's VM digest differed for `logf` and `sin`.) The checker itself flags 2,099 of 3,000 unfused
`a*b+c` results, and agrees with the PC's hardware on every special triple. So `RUSTYAC_MATH=pure`
now gives the PC's bits on the VM, and condition 6 of part 2c is met.

### Checks

**The PC's keys.** The old and the new `install_key`, run side by side on the PC: both
`a20661e04fa6997a`; the new one with `maths_path=SSE2`: `b46551863648edea`. So the PC's kept results
keep their keys, and the VM's can never stand in for them. (`fresh`'s recordings are new anyway
after this part: `salt_of car_oracle` hashes the oracle's own source, which part 1 had to change, as
every change to `car_oracle` does. The quick run recorded 21 of them again.)

**The quick run** (`run_all.sh --quick`, 10 GB free, so two lanes joined), stopped by Claude Code
for low memory (`ai_page`'s headless Chrome GPU process held about 8 GB) after these rows:

| area | cases | identical | time |
|---|---|---|---|
| physics | 4 | 4 | 4m30s |
| dumps | 6 | 6 | 4m47s |
| render | 18 | 18 | 12m08s |
| shaders | 4 | 4 | 3m22s |
| net | 16 | 16 | 5m28s |
| gui | 29 | 29 | 17m05s |
| python | 1 | 1 | 0m15s |
| browser | 15 | 15 | 16m08s |
| fresh | stopped after 50 | 50 | |
| ai_page | stopped after 2 | 2 | |

The rest, in F2's second session (`run_all.sh --quick fresh ai_page csp sound replays launcher`,
15 GB free, after the main folder's run that held the lock):

| area | cases | identical | time |
|---|---|---|---|
| fresh | 63 | 63 | 21m54s |
| ai_page | 9 | 9 | 26m40s |
| csp | did not build (below), then alone: 49 | 47 | 6m11s |
| sound | 4 | 4 | 0m35s |
| replays | 6 | 6 | 4m44s |
| launcher | 1 | 1 | 0m46s |

Memory stayed above 2.5 GB free this time (ai_page ran beside fresh only).

- **csp_oracle did not build**: part 1 made `car_oracle`'s `acs.rs` (which `csp_oracle` includes by
  path) call `crate::repo_root()`, and `csp_oracle` had none (part 1's clippy list missed it).
  Fixed with the same `repo_root()` as the other oracles.
- **csp's two DIFFERENT cases are not F2's**: `refused off` and `refused csp` are DIFFERENT in the
  main folder's quick run of the same day (master, Task C5a) as well. `acd_check` finds
  `cardata/rss_formula_rss_3_v6/car.ini` (ignored, extracted) no longer equal to the car's
  `data.acd` (car.ini changed 2026-11-17 23:14, after the archive), so that car fails both
  modes and the `RESULT` line is missing. Left for Super / the CSP tasks.
- **physics found no `.mcrec` in this worktree**: `oracle` is a junction here, and `find oracle
  -name '*.mcrec'` does not enter a starting point that is a link. The area ran only the
  `oracle/car*/` folders (part 1's "4 cases"; master, a real folder, runs 9). Now `find oracle/`;
  the keys are the same.

Clippy `-D warnings --all-targets`: the workspace (native, wasm32-unknown-unknown, wasm32-wasip1),
`math_proof`, `audio_oracle` (one old `needless_range_loop` fixed), `chassis_compare`,
`tyre_oracle`, `render_oracle`, `csp_oracle`: clean. `car_oracle`, `ode_oracle` and `sctm_oracle`
failed on errors that were there before (newer lints, none on a line F2 changed); they now have
crate-level `#![allow(...)]` for exactly those lints, with a comment, since rewriting an oracle's
arithmetic or extern declarations for a lint can move its bits: `car_oracle` `dead_code`,
`clashing_extern_declarations`, `assign_op_pattern`, `excessive_precision`, `if_same_then_else`,
`manual_clamp`, `manual_is_multiple_of`, `missing_transmute_annotations`, `needless_late_init`,
`needless_range_loop`, `ptr_eq`, `redundant_closure`, `too_many_arguments`, `type_complexity`,
`unnecessary_cast`, `unnecessary_sort_by`, `wrong_self_convention`; `ode_oracle`
`manual_is_multiple_of`, `needless_range_loop`, `neg_cmp_op_on_partial_ord`,
`wrong_self_convention`; `sctm_oracle` `needless_lifetimes`, `redundant_guards`. All three are clean
with them.


## F2 part 2: prebuilt programs and the copy

### Prebuilt programs (`RUSTYAC_PREBUILT=1`)

`bash tools/proofs/prebuild.sh` (on the PC) builds every program the areas run, with the areas' own
`cargo` commands so the bytes are the same, and writes `target/prebuilt/`:

- `manifest.txt`: SHA-256 and repo-relative path of 24 files, 113 MB: the five oracles
  (`car_oracle`, `chassis_compare`, `render_oracle`, `audio_oracle`, `csp_oracle`); `rustyac`,
  `replay_hash`, `track_survey`, `acd_check`, `rustyac-launcher`; the examples fresh, shaders and net
  run (`start_camera_check`, `steam_check`, `mirror_check`, `achievements_check`, `shader_table`,
  `shader_proof` with `wgpu`, `net_golden` native and wasm32-wasip1); wasmtime; shaders' test program
  `dxbc_reflect`; `launcher_oracle` (restored from NuGet on the PC); the dumps area's last release.
- `src/<tool>.txt`: the `git ls-tree` / `git diff` lines `salt_of` hashes, for every tool, so a
  machine without the repository's history makes the same keys.
- `tests/`, `dumps_tag`.

With `RUSTYAC_PREBUILT=1`, `run_all.sh` and `lib.sh` source `tools/proofs/prebuilt/env.sh`: it puts
`tools/proofs/prebuilt/` first on PATH, checks every manifest file once a run (a changed file stops
the run), and sets `DUMPS_OLD`. There, `cargo` is a stand-in: `build` does nothing, `run
--bin|--example <name>` runs the built exe, `test --test <name>` runs the test program from the
package's folder (as cargo does), anything else stops with "not prebuilt"; `dotnet` does nothing.
A PATH stand-in (not a shell function) also reaches scripts the areas start with `sh`
(`tools/net_gold/check.sh`). Nothing else in an area needs git (only `--changed` does).

**Not prebuilt** (they stay on the PC anyway): browser (`cargo test` of `web_pack`'s unit tests,
the wasm builds of `replay_hash` and `math_proof`) and ai_page (Chrome, the web pack). Helper scripts
outside the areas (`render_wgpu.sh`, `launcher_record.sh`) still need cargo. The dumps area's old
release must have been built once on the PC (`prebuild.sh` says so when it is missing).

Checked on the PC (`RUSTYAC_PREBUILT=1 run_all.sh --quick physics shaders net launcher dumps fresh`):
every area IDENTICAL (4, 4, 16, 1, 6, 63 cases), 21 min. The keys are the PC's: `salt_of car_oracle`
from git and from `src/car_oracle.txt` are both `c1af0ef52f0411a5`, and fresh's key listing
(`PROOF_LIST_KEYS`) gives the same 26 keys in both modes, every one already kept.

### The copy (`tools/proofs/farm_sync.sh`)

`FARM_SHARE=//<vm>/rustyac-farm bash tools/proofs/farm_sync.sh [programs] [physics]` copies over SMB
with robocopy (`/MT:8 /J /XO`; a file of the same size and time is skipped), into `<share>\repo` (for
`RUSTYAC_REPO`) and `<share>\ac` (for `AC_ROOT`). Only `repo\tools\proofs` and `repo\target\prebuilt`
are mirrored (deletions); everything else only gains files, so results made on the VM stay. Every
source is named in the script, so nothing under a served folder goes. With no part named it runs
all four:

- `programs`: the manifest's files folder by folder with the PC's `msvcr120.dll` and
  `vcruntime140.dll` beside each (F1) and the DirectX June 2010 DLLs `acs.exe` imports
  (`d3dcompiler_43.dll`, `d3dx11_43.dll`, `xinput1_3.dll`: a plain Windows lacks them, and the game
  side stops with "d3dcompiler_43.dll not found"), `tools/proofs`, `tools/net_gold`,
  `target/prebuilt`, `cardata/`, AC's `system/` and `acs.exe`: 187 MB the first time (+33 MB for the
  DirectX DLLs), about 1 MB after.
- `physics`: every `oracle/car*/` folder, the top level of every folder holding a `.mcrec`, the
  tracks the recordings name (`track=` in their header: bahrain_international_circuit, imola,
  ks_drag, ks_vallelunga, magione, mugello, spa) and every car's `collider.kn5`: 15.8 GB.
- `ac`: the rest of the install, all of AC's folder but `sdk/` (Kunos' editors, which nothing
  reads): 71.8 GB in all (robocopy `/L`), less what `physics` already sent.
- `repo`: `oracle/` (15.1 GB) and `re/gold` without `web/` (22.5 GB; the browser's kept results stay
  on the PC with the browser), plus the two repo files areas read besides the programs: python's
  test apps (`tools/render_oracle/py_apps`) and `tools/csp_oracle/src/main.rs` (csp.sh takes its
  race.ini from there).

`oracle/`, `re/gold` and AC's folder hold no junctions (checked), so robocopy's following of
junctions copies nothing twice. The rest of the one-time copy (`ac` and `repo`, about 90 GB not yet
there) is Super's to start, e.g. overnight, from Git Bash in this worktree:
`FARM_SHARE=//<vm>/rustyac-farm bash tools/proofs/farm_sync.sh programs ac repo` (about 5 h at
5 MB/s; it can be stopped and started again, robocopy skips what is there).

The share must be given as `//<vm>/rustyac-farm` (or `\\<vm>\...`); anything else stops the script,
and so does a share without a `repo` folder. (A lost backslash in the first try made robocopy write
8.8 GB to `C:\<vm>\...` on the PC; Super deleted it.) F1's padded folders on the VM were renamed to
`repo` and `ac` to reuse what was there.

**Speed: 5.3 MB/s** (the physics part, 15.8 GB in 47 min; the first GB, `oracle/car`, 974 MB in
175 s), over the laptop's Wi-Fi. That is under the 20 MB/s the plan wanted, so the one-time copy
stopped after the physics part: the other ~85 GB would take about 4.5 h at this speed. Many small
files are slower still (`cardata/`, 11 MB in 113 s). Ethernet first, then the rest.

## F2 part 3: the areas on the VM

### The full physics area (F1's condition 3)

`RUSTYAC_PREBUILT=1 run_all.sh --full physics` in the VM's copy (`re/scratch/taskF2/vm_run.sh`:
the farm's own home folder and lock, `RUSTYAC_REPO` and `AC_ROOT` pointing into it), 121 cases (the
area has grown since F1's 113), against the recordings the game made on the PC:

| maths in the port | identical | different | error | time (one lane) |
|---|---|---|---|---|
| MSVCR120.dll (the default: SSE2 bodies on the VM) | 94 | 25 | 2 | 60m38s |
| `RUSTYAC_MATH=pure` (pure.rs, the FMA3 bodies) | **121** | 0 | 0 | 93m25s |

**On the DLL path the PC's recordings do not hold.** 25 recordings differ, each by one ulp first
in a wheel rotation or a body's orientation late in the drive (`pt_protect` step 3317 of 3334:
`tyre.lr.localWheelRotation.M23` 0xbf78718b against 0xbf78718c): sinf/cosf inputs where the SSE2
and FMA3 bodies disagree, which F1's 9 quick cases happened not to meet. They were
`car_pt/pt_protect`, `car_tight_stops/random`, `car_wc_488/wc_stops` and 22 `.mcrec` recordings
(the pile-up, rear-end and field drives of `multi` and `t24b`, the AI drives of `t27`/`t27f`, the
race-rules drives of `t28`). Every one is IDENTICAL with `RUSTYAC_MATH=pure` (`car_pt` alone first,
then the whole area), which needs part 1's correctly rounded fma. So `physics.sh` now sets
`RUSTYAC_MATH=pure` whenever `maths_path` is not FMA3: every recording was made on FMA3, and the
port then takes the exact Rust copies of those bodies. **Areas where the game runs in the same
process (csp, render, gui, python, sound, fresh's new recordings) must keep the DLL path** there,
since the game takes SSE2 too; areas against kept results need the same switch when they move.

**What each machine proves.** On the VM the physics area proves the **pure** maths path (pure.rs,
the one the browser takes); the PC's runs keep proving the DLL path (MSVCR120.dll's FMA3 bodies, the
desktop's default). Neither stands in for the other, so a `--farm` full run says so in its summary
line (part 4).

The two ERRORs were the harness: `t27/v6` and `t27f/v6_line` drive an old AI line kept in
`re/gold/roots/t27_v6`, and their header's `ai_folder` is the PC's absolute path. A recording
replayed on another machine now finds that folder in its own repository (the longest tail of the
path that exists there, `engine/record.rs`, with a unit test), as `track_folder` already did with
the game's folder; `farm_sync.sh physics` sends `re/gold/roots`.

**A `find` through a junction.** In the browser worktree `oracle/` is a junction, and `find oracle
-name '*.mcrec'` does not enter it (Git's `find` takes a junction for a link), so physics there
skipped every recording without a word; `find oracle/` does. The other area scripts were checked
for the same: only physics had it (fresh's `find oracle/t24b oracle/t28 oracle/t28b` starts below
the junction and finds the same 62 files as in the main folder; the rest walk `target/` or AC's
folder, which hold no junctions, nor do `oracle/`, `re/gold` or AC's folder inside).

**Time.** Pure maths is slower on the Westmere (libm's software fma, no FMA instruction): 93 min in
one lane, where the PC's last full run took 85 min for this area in its four-lane run. The area's
cases are independent, so part 4's lanes split it (the longest case, `rc_weekend_q`, is 6m44s).

### Session 0 and the logged-in session (step 2)

With only what the cases read copied (`re/scratch/taskF2/sync_trial.sh`: the default car, AC's
`cfg`, `apps`, `launcher` and `content/{fonts,gui,sfx,weather,texture,driver,objects3D}`, 2.1 GB),
three runs through `vm_run.sh` (`re/scratch/taskF2/vm_trial.sh`): render (`r20_01`, and quick's
`overlays ks_drag/drag400`), gui (`base`) and the launcher's twelve lists:

| where it runs | render | gui | launcher |
|---|---|---|---|
| ssh (session 0, no desktop) | ERROR 2 of 2: `D3D11CreateDeviceAndSwapChain` 0x887A0022 (DXGI_ERROR_NOT_CURRENTLY_AVAILABLE), then the game's code crashes | ERROR: the same | DIFFERENT 10 of 12: the screen's modes come back empty (`videomodes`), the audio page's answers differ, `probe` crashes the oracle (an access violation in the launcher's code) |
| a scheduled task in the logged-in user's session | **IDENTICAL 2 of 2**, 26 s | **IDENTICAL**, 23 s | **IDENTICAL 12 of 12**, 2m48s |

So **session 0 cannot run the WARP areas or the launcher; the logged-in session can.** The VM's
user stays logged in at the console (Super leaves it so), and the runs start as a scheduled task
with `/IT` (interactive only: it runs in that user's session, on its desktop, and needs no password):

    schtasks /create /tn <name> /tr "C:\rustyac-farm\f2\trial_task.cmd <suffix>" /sc once /st 23:59 /it /ru <user> /f
    schtasks /run /tn <name>

made over ssh (`/sc once` with a time that never comes: `/run` starts it). `schtasks /change` asks
for the password even then (it hung the ssh call): make the task again with `/create /f` instead.
The task's output goes to a log file the ssh side reads. Part 4's farm runner starts render, gui,
python, yebis_sweep, shaders and the launcher this way; CPU-only areas can stay on plain ssh (the
physics area ran there).

### The rest of the copy

Super ran `farm_sync.sh programs ac repo` overnight; robocopy's logs (`target/farm_sync/`) show it
finished with nothing failed: `ac` 65.4 GB copied (71.8 GB with what was there) in 3h37m, `repo`
2.1 GB of `oracle/` (13.0 GB were there from the physics part) and 22.5 GB of `re/gold` in 1h14m:
**about 5 MB/s**, the Wi-Fi speed of part 2. The `csp` part (CSP's frozen build, 214 MB) later went
at 9.3 MB/s. The VM's disk: 137 GB used of 250 GB.

### Every other area on the VM (step 3)

Each area's quick run in the VM's copy (`re/scratch/taskF2/task.sh` through `task.cmd`: the scheduled
task in the logged-in session for all of them, CPU areas too, so the farm needs one way to start a
run), against the PC's last quick run (part 1, four lanes on both):

| area | on the VM | VM time | PC time | where it runs now |
|---|---|---|---|---|
| physics | 9/9 IDENTICAL (pure maths) | 11m43s | 4m30s (4 cases then) | farm |
| fresh | 63/63 IDENTICAL | 45m22s | 21m54s | farm |
| csp | 49/49 IDENTICAL | 14m02s | (did not build in part 1) | farm |
| sound | 4/4 IDENTICAL | 0m53s | 0m35s | farm |
| render | 18/18 IDENTICAL | 24m00s | 12m08s | farm |
| gui | 29/29 IDENTICAL | 25m46s | 17m05s | farm |
| python | 1/1 IDENTICAL | 0m25s | 0m15s | farm |
| replays | 6/6 IDENTICAL | 7m36s | 4m44s | farm |
| net | 16/16 IDENTICAL (native ways on pure maths) | 8m22s | 4m35s | farm |
| launcher | 1/1 IDENTICAL | 1m28s | 0m46s | farm |
| dumps | 6/6 IDENTICAL | 5m49s | 5m12s | farm |
| yebis_sweep | 7/7 IDENTICAL (a 7-row sample, `SWEEP_ONLY`) | 3m06s | 1m23s (the same rows) | farm |
| shaders | **2/4: `shader_proof warp_flat` and `swift_flat` DIFFERENT** (84 and 73 rows of the tables) | 1m09s | 3m22s | **PC** |
| browser, ai_page | not tried (Chrome, Node, fixed ports) | | | **PC** |

The first whole quick run took 1h05m on the VM (12 areas, four lanes) against 0h29m on the PC.

- **Fresh** made every recording again with the game on the VM: the kept recordings' keys now
  hold the maths path (part 1), so none of the PC's stood in, and the VM keeps its own (SSE2) ones in
  its `re/gold/fresh/rec`; the next run there reuses them. The game and the port both took the DLL's
  SSE2 bodies, and every case was IDENTICAL.
- **Shaders stays on the PC.** Its tables must be Task 38b's again, row for row, and WARP's and
  SwiftShader's own code takes another path on a processor without AVX: the pixels differ, not the
  port. (Tables made on the VM would prove nothing about Task 38b's.) It is 3 minutes on the PC.
- **WARP's speed** (F1 left it unmeasured): render 2.0 ×, gui 1.5 ×, the YEBIS sweep 2.2 × the PC's
  time in the same lanes. The full sweep, 528 rows and 93 min on the PC, would be about 3.4 h in one
  lane on the VM, so part 4 splits it into lanes too.

What the runs found and what was fixed (each a farm-only cause; the PC's runs are unchanged):

- `lib.sh`'s `install_key` said "once a run: run_all.sh exports it", but nothing exported it, and
  its callers run in `$(...)` subshells, so every key walked AC's `system/`, `content/cars` and
  `content/tracks` again: 22 s each on the VM (Git's `find` over 36,585 files), four lanes at once.
  It now runs once an area, when `lib.sh` is sourced.
- **Drives name the PC's AC folder**: 162 of the 228 `oracle/game/*.ryin` drives hold the track's
  absolute folder. `rustyac.exe` (and the dumps area's old release, which no fix can reach) stopped
  with "the track ... was not found", and dumps kept an empty dump. `lib.sh`'s `drive()`, which
  already rewrote a drive's Documents path into a copy, now also rewrites the PC's AC folder into
  `$ac` when `AC_ROOT` is elsewhere (nothing changes on the PC); sound's end-to-end replay goes through
  it too. The four empty dumps the VM kept were deleted.
- `audio_oracle --tape` took the tape's absolute track folder as it was; it now falls back to the
  game's own `content/tracks/<name>` when that folder is missing, as `--tapes` already did.
- **Net against the PC's goldens** differed on the native ways (desktop, browser, gateway) late in
  two drives: the physics area's SSE2-against-FMA3 split. `net.sh` sets `RUSTYAC_MATH=pure` off FMA3,
  as `physics.sh` does; the wasm way already takes pure maths.
- wasmtime stops with "config file not specified and failed to get the default" when
  `USERPROFILE` points at a folder without `AppData\Roaming` (the farm's own home): the farm runner
  makes it.
- **Python**: AC's `python33.dll` needs `MSVCR100.dll`, which a plain Windows lacks, and
  `rustyac-python` loads it with `LOAD_WITH_ALTERED_SEARCH_PATH`, so the exe's folder is not searched:
  `farm_sync.sh` sends `MSVCR100.dll` with the other DLLs and the farm runner puts render_oracle's
  folder on `PATH`.
- **csp** reads `re/csp/fakeac` (CSP's frozen build over junctions into AC's folder):
  `fakeac_csp.ps1` takes `AC_ROOT`, `farm_sync.sh csp` sends the frozen build and the script, and it
  was made once on the VM (`AC_ROOT=C:\rustyac-farm\ac powershell -File tools\csp\fakeac_csp.ps1` in
  its repo folder). Remove it there only with `fakeac_csp_rm.ps1`, as on the PC.
- render's `r20_13` / `r20_21` read `crates/rustyac-render/tests/data/*.pose`: `farm_sync.sh repo`
  sends that folder.
- New tags on `master` (v1.1.2, v1.1.3) changed the dumps area's old release under the run:
  `prebuild.sh` needs that release's exe built once on the PC (a quick dumps run) before it writes
  the manifest.

Checked on the PC after the changes: `run_all.sh --quick sound net dumps`, 4/4, 16/16, 6/6
IDENTICAL, and dumps again against v1.1.3, 6/6.

**Locked, no remote desktop** (how the VM sits, logged in at the console by Autologon): every run
above ran in the console session with no remote desktop connected. With the session locked (a `/IT`
task running `rundll32 user32.dll,LockWorkStation`, `re/scratch/taskF2/lock.cmd`; LogonUI then shows
in session 1), render's `r20_01` (3 cases) and gui's `base` still ran IDENTICAL through the
scheduled task: a locked desktop still makes WARP's windows and swap chains. Nothing more is needed.

## F2 part 4: `run_all.sh --farm`

    bash tools/proofs/run_all.sh --quick --farm     (or --full --farm, or with area names)

needs the farm's settings (F3: `%USERPROFILE%\.rustyac\farm.env`, see "Setting up the farm"), with `FARM_HOST` (ssh's user@host, key
login; the user is also the scheduled task's) and `FARM_SHARE` (`//<vm>/rustyac-farm`); the
environment wins over the file. The pieces (`tools/proofs/`):

- **Which areas go.** `areas.map`'s `@farm` line: physics, fresh, csp, sound, render, gui, python,
  replays, net, launcher, dumps, yebis_sweep (part 3: every area but shaders, browser and ai_page,
  which run on the PC at the same time, in its usual lanes). `areas.sh` skips that line.
- **Before any lane starts** (`farm.sh`'s `farm_up`, inside the PC's proof lock, which so covers the
  whole farm run): ssh answers, the share answers, no earlier farm run is still running there, and
  `prebuild.sh` builds every program the VM runs. The dumps area's old release now comes from
  `old_exe.sh` (built once per tag, also by `dumps.sh`), so **a new release tag on master needs
  nothing by hand**: prebuild builds that release's exe and the copy sends it.
- **Beside the PC's lanes** (`farm_go`, in the background): `farm_sync.sh programs repo csp ac`
  (only what changed: about 30 s to 2 min when little did, `ac` alone 15 s), then the run's
  arguments and lanes into the share's `run\`, then over ssh
  `schtasks /create ... /it /ru <user> /f` and `/run` of `farm_task.cmd`, which runs `farm_run.sh`
  in the logged-in session (part 3: session 0 cannot run WARP or the launcher). `farm_run.sh` is
  `vm_run.sh` made real: the roots from its own place (`<farm>\repo`, `<farm>\ac`, `<farm>\home` with
  `AppData\Roaming`), `RUSTYAC_PREBUILT=1`, render_oracle's folder on `PATH`, then the VM's own
  `run_all.sh` with `PROOF_LANES`; last it lists the kept files the run made (`new_kept.txt`) and
  writes `exit`.
- **Lanes and shards.** `run_all.sh` takes other lanes from `PROOF_LANES`; a lane word `<area>.<i>`
  runs shard i of n of the area's cases (`lib.sh`'s `PROOF_SHARD=i/n`: a case runs when its quick key
  hashes, `cksum` mod n, to i), into its own `target/proofs/<area>.<i>` (an area deletes its out
  folder when it starts). Once every lane is done the shards are merged into the area's folder, log
  and row (the time: the longest shard's); a case found in two shards (a verdict outside `keep()`)
  becomes an ERROR, so nothing is counted twice unseen. The VM's default lanes (`farm.sh`, eight;
  `FARM_LANES` sets others): quick `fresh.1|fresh.2|gui.1 sound|gui.2 python launcher|render.1
  replays|render.2 dumps|csp|physics net`, full `fresh.1 sound python|fresh.2 launcher csp|fresh.3
  dumps|gui.1 physics.1|gui.2 physics.2|render.1 yebis_sweep.1|render.2 yebis_sweep.2|net replays
  yebis_sweep.3`. Checked on the PC: quick physics as three shards, 4 + 4 + 1 cases, merged 9/9
  IDENTICAL, nothing vanished.
- **Waiting, bounded.** Every 30 s the PC mirrors the VM's status files (`run.status`, each area's
  `cases.log`, `times.tsv`, `now`, rows) into `target/proofs/farm/`, which `proof_tui` reads: the
  VM's lanes show in a row below the PC's ("VM lane n") when the screen has room, beside them when
  not. The farm counts as failed when the task has not started the run in 5 min (no logged-in
  user), the share stops answering for 10 min, or no new verdict comes for `FARM_STALL` minutes
  (60); the task is then ended.
- **Results back.** For every area the VM finished, its folder (without `own`, `root`, `rec`,
  `frames`, `drives`), log and row come into the PC's `target/proofs/` as if it had run here, its
  "where" is VM, and the ERROR-again-alone pass is the VM's own. The kept files the VM made in this
  run (`re/gold/fresh/rec`, `re/gold/dumps`: its SSE2 keys) come back too, each folder's `done` last.
  Only those: a whole-tree copy of the VM's kept results (tried first) would bring back what
  `archive_kept.sh` moved off the PC (2 GB of old keys there), at Wi-Fi speed.
- **Fallback.** An area the farm could not run for a farm reason (VM off or no answer, the copy,
  ssh, the task not starting, a stall, "did not run" there, its results not coming back) runs on
  the PC after the PC's own lanes, and the table says so: `PC (VM: <reason>)`. A DIFFERENT or ERROR
  case on the VM is a result and stays one. Checked: with ssh refused, sound ran on the PC from the
  start ("the VM does not answer ssh ..."); with a wrong farm folder the task never started, and
  after the 5 min sound ran on the PC ("the VM's scheduled task did not start the run in 5 min").
- **The table** of a `--farm` run has a "where" column, and its last line says
  `physics on the VM proves pure.rs's maths, not MSVCR120.dll's` when physics ran there (part 3).

First end-to-end run (`--quick --farm sound python launcher`): 4/4, 1/1, 1/1 IDENTICAL on the VM,
its run 2 min.

## F2 part 5: the real runs

### `run_all.sh --quick --farm`

All 14 areas, 286 cases, against the PC's own quick run of the same day (main folder, 0h59m):

| area | where | cases | on the farm run | farm run time | PC-only run time |
|---|---|---|---|---|---|
| physics | VM (pure maths) | 9 | 9 IDENTICAL | 3m14s | 1m15s |
| fresh | VM, 2 shards | 63 | 63 IDENTICAL | 28m12s (24 + 39 cases) | 19m14s |
| csp | VM | 51 | 51 IDENTICAL | 23m31s | 9m40s |
| sound | VM | 4 | 4 IDENTICAL | 0m56s | 0m27s |
| render | VM, 2 shards | 18 | 18 IDENTICAL | 17m40s | 8m39s |
| gui | VM, 2 shards | 29 | 29 IDENTICAL | 29m32s (17 + 12) | 18m59s |
| python | VM | 1 | 1 IDENTICAL | 0m48s | 0m18s |
| replays | VM | 6 | 6 IDENTICAL | 10m05s | 5m53s |
| net | VM | 16 | 16 IDENTICAL | 13m24s | 9m00s |
| launcher | VM | 1 | 1 IDENTICAL | 2m57s | 1m15s |
| dumps | VM | 6 | 6 IDENTICAL | 4m00s | 3m04s |
| browser | PC | 18 | 18 IDENTICAL | 17m33s | 19m58s |
| ai_page | PC | 9 | 9 IDENTICAL | 28m01s | 27m13s |
| shaders | PC | 4 | 4 IDENTICAL | 1m07s | 3m40s |
| **wall clock** | | **286** | **0 different, 0 ERROR, 0 vanished** | **0h45m** | **0h59m** |

The farm half: the copy 29 s (little had changed), the VM's run 31m17s (eight lanes; its longest,
`gui.1`, 29m32s; part 3's quick on the VM took 1h05m in four lanes), the results back 4 min, nearly
all of it the 99 kept files the VM made for cases added on master since part 3 (913 MB of SSE2
recordings at Wi-Fi speed, once). **The long pole is now the PC's browser + ai_page lane** (45m34s),
not the VM: with nothing new to bring back, a quick farm run is about that lane. Running browser and
ai_page in two lanes (their ports already differ) would bring it to about the VM's 31 min, but two
headless Chromes at once (ai_page's GPU process reaches about 8 GB) need their own proof first; to
try it: `PROOF_LANES="browser|ai_page shaders" run_all.sh --quick --farm` (the PC's lanes only; the
VM's come from `FARM_LANES`).

### `run_all.sh --full --farm`

Not run yet: it waits for Super's go. Expected: the PC half (browser, ai_page, shaders) about
2 h; the VM half about 2.5-3 h from the per-area full times (P1) times part 3's VM ratios, split over
the eight full lanes. The first one is longer: the VM has no full-run kept recordings of its own yet
(fresh records every case again with the game, SSE2 keys), and they come back to the PC once, about
15 GB at Wi-Fi speed (~50 min, ~15 GB of the PC's disk). So about **3.5-4 h the first time, then
about 3 h**, against about 6.5 h on the PC alone.

## F3: the settings every checkout reads, the run's 4 non-identical cases

Super's first `--full --farm` (main folder, 2026-11-19, 7h11m) ran **every** farm area on the PC:
F2 had written its settings to the worktree's `target\farm.env`, the main folder had none, and
`farm_up` fell back to the PC area by area without a word until the summary's "where" column.

### Setting up the farm

The settings live in **`%USERPROFILE%\.rustyac\farm.env`**, beside the proof lock, so every
checkout of the PC reads the same file (never in the repo: it names the VM). `target\farm.env` is
no longer read (one place only; the worktree's was moved there). On a new PC or user:

1. The VM as in F2 parts 3-4 (its farm folder `C:\rustyac-farm`, shared as `rustyac-farm`; its
   user logged in at the console, ssh with key login from the PC).
2. `%USERPROFILE%\.rustyac\farm.env`, two lines (a bash file; the environment wins over it):

       FARM_HOST=<user>@<vm>
       FARM_SHARE=//<vm>/rustyac-farm

3. Once: `FARM_SHARE=//<vm>/rustyac-farm bash tools/proofs/farm_sync.sh` (the whole copy, hours over
   Wi-Fi); later runs send only what changed themselves.

A new checkout of the same PC needs nothing: `run_all.sh --farm` finds the file.

**`--farm` without settings, or with a VM that doesn't answer**, now stops before any area runs,
exit code 2, with one line, e.g.

    the farm isn't set up: no FARM_SHARE in .../.rustyac/farm.env; run without --farm (or with --farm-fallback to run its areas here), or set it up: docs/proof_farm.md, "Setting up the farm"

The same for: no `FARM_HOST`, ssh not answering, the share not answering, an earlier farm run still
running on the VM, `prebuild.sh` failing. **`--farm-fallback`** is `--farm` with the old behaviour
(those areas run on the PC, the reason in the "where" column). A failure **during** the run (the
copy, the task not starting, a stall, results not coming back) still falls back area by area, as
in F2. Checked: no file and a dead host, each `exit=2` before any area, the lock released.

### The run's 4 non-identical cases (all on the PC)

**fresh and yebis_sweep: "the default own folder" DIFFERENT (the guard was right).** Between 16:28
and 16:57 `target/release` of the main folder got `cfg/launcher.ini` (the launcher's window
settings), `cfg/audio.ini`, `controls.ini` and the rest of a first start (16:28), a new
`setups/ks_ferrari_488_gt3/` (16:29), then `race.ini` (ks_ferrari_488_gt3 at ks_laguna_seca,
practice), `camera_*.ini`, `acos.ini` and a 1:25.223 lap in `personalbest.ini` (16:57): rustyAC's
launcher and game **started by hand from the main folder** and driven for half an hour. It was not
a task: no Claude session ran a command in that window (C5d's only watched its queued csp run, which
waited for the lock), no proof case drives a 488 GT3 at Laguna Seca, and the only areas running then
were fresh and yebis_sweep, which both caught it. The proofs' own runs all have `--cfg-dir` and
`--documents`, so their verdicts were not touched; the guard can't tell who wrote. Fix: the guard's
line now names the newest file written there and its time, and says that a rustyAC started by hand
from the checkout's `target/release` trips it too (**play from another folder during a proof run**).
Nine old `tools/web_checks/` scripts (task38j-44c) still start `target/release/rustyac.exe` without
`--cfg-dir`; none ran here, so they are left as they are.

**sound: `ks_ferrari_f2004 spa_kerb_strike cockpit` and `ks_ferrari_250_gto trk_kerb_strike
cockpit` DIFFERENT (FMOD crashed on every try; the proof script called it DIFFERENT).** The crashes
are the known FMOD 1.08.12 bug in the clock-less setup (docs/port/leftovers.md, the FMOD row; sites
`fmod64.dll+0x8fc37` and `+0xf5776`). `audio_oracle compare` already makes a crashed side again (four
tries a side) and recognised these crashes; what failed is that each side crashed four times. Measured
here, run by run (`re/scratch/taskF3/fmod_side.sh`):

| tape, side | crashed |
|---|---|
| f2004 spa_kerb_strike, port | 12 of 30 (40 %) |
| f2004 spa_kerb_strike, game | 1 of 30 |
| 250_gto trk_kerb_strike, game, idle PC | 2 of 30 |
| 250_gto trk_kerb_strike, game, every CPU busy | 5 of 30 |

At 40 % a compare fails about 0.4⁴ = 2.6 % of the time (1 of 10 compares here), and load raises the
rate. `sound.sh` then gave `verdict` the last line of audio_oracle's output, a stack frame, so the
crash became a DIFFERENT and never got run_all.sh's "an ERROR runs once more, alone, after all lanes".
Fix (`sound.sh`): the crash line goes to the verdict, which makes it an **ERROR**, run again alone;
a real difference stays DIFFERENT (both checked on the run's text). The farm runs the same `sound.sh`
under the VM's own `run_all.sh`, which reruns its ERROR cases the same way.

**Rerun** (`re/scratch/taskF3/rerun.sh`, the worktree, its own output): sound's 2 cases IDENTICAL,
fresh's `conditions spa_cold_green_wind` and yebis_sweep's `x_pp3_g0_d0_r1` IDENTICAL with the guard
quiet (no `default own folder` line). In 10 more compares each, f2004 spa_kerb_strike was ok 9 times
(5 with a crashed run made again) and failed once with every try crashed; 250_gto 10 of 10 ok.

### The copy leaves out the PC's kept game results

The first try of step 3's quick farm run spent its first 17 min copying `re/gold` to the VM: the
main folder's full run had recorded fresh again (16 GB of new kept folders). Every kept folder's key
(`fresh/rec` through `salt_of`, `dumps` through its `key_salt`) holds `install_key`, which holds the
maths path off FMA3, so the VM never reads one the PC made. `farm_sync.sh`'s repo part now leaves out
`re/gold/fresh/rec` and `re/gold/dumps` (the VM keeps its own there; the copy never deletes). That
run was stopped and made again. It then showed why the old `re\gold\web` exclusion had never
worked either: robocopy's `/XD` takes a folder's name or its **full** path, and `cygpath -w` gave
a relative one; the excludes are now `cygpath -aw`. Checked with `/L` (`re/scratch/taskF3/gold_l.sh`):
the step now lists 61 files, 248 MB.

### `--full --farm`: lanes and time

From the full run's PC area times (`rows/*.secs`, minutes: physics 101, render 136, gui 111, dumps
102, yebis_sweep 108, replays 54, net 44, csp 8, launcher 5, sound 4; fresh 354 recording every case
again, 205 from kept results (P1)) times part 5's VM/PC ratios (physics 2.6, fresh 1.47, csp 2.4,
render and yebis_sweep 2.0, gui 1.56, replays 1.7, net 1.5, dumps 1.3), the VM's area time is about
1,570 min, packed longest-first into 8 lanes with shards (all lanes within 3 min of each other):

- **Later runs** (the default `farm_lanes_full` in `farm.sh` now), about **3h20m** on the VM:

      yebis_sweep.1 gui.1|yebis_sweep.2 gui.2|physics.1 replays.2 csp|physics.2 net|render.1 dumps.3 launcher|render.2 replays.1 sound python|fresh.1 dumps.1|fresh.2 dumps.2

- **The first one**: the VM has no full-run kept recordings, so fresh records every case again
  (about 520 min of VM time, 1,790 in all); with these lanes, about **3h50m**:

      FARM_LANES="fresh.1 net replays.1|gui dumps.1 sound|physics.2 dumps.3 replays.3 csp|fresh.4 render.3|fresh.3 render.2|fresh.2 render.1|yebis_sweep|physics.1 dumps.2 replays.2 launcher python" bash tools/proofs/run_all.sh --full --farm

  plus about 50 min to bring the VM's new kept recordings back (about 15 GB at Wi-Fi speed): about
  **4h40m**, against 7h11m for this run on the PC alone.
- **A range, not a number**: F3's quick farm run (below) ran the WARP areas slower than part 5's
  (render 28m28s against 17m40s, gui 40m02s against 29m32s, same cases), so with its ratios the
  same lanes come to about **4h55m** later and **5h55m** the first time (the yebis_sweep lane then
  the longest). The first full farm run's own area times are what to pack the lanes from next.
- The PC half beside it (browser 116, ai_page 32, shaders 7 min, one lane) is about 2h35m, inside
  both. Shards split an area's cases by key, not by time, so a lane can be off by a few minutes; the
  physics ratio comes from 9 quick cases on the pure maths path and is the least certain.

### `run_all.sh --quick --farm` (step 3)

From the worktree on `web` (the main folder is on master, without F3, and C5e was working there;
after the merge it reads the same `%USERPROFILE%\.rustyac\farm.env`), with no `target/farm.env`
in the checkout any more: every farm area ran on the VM, **288 of 288 IDENTICAL, 0 ERROR, 0h45m**.

| area | where | cases | time |
|---|---|---|---|
| physics | VM | 9 | 14m38s (its side-by-side multi recorded again: 753 s, once) |
| fresh | VM, 2 shards | 63 | 29m47s |
| csp | VM | 53 | 38m33s |
| sound | VM | 4 | 1m00s |
| render | VM, 2 shards | 18 | 28m28s |
| gui | VM, 2 shards | 29 | 40m02s |
| python | VM | 1 | 0m49s |
| replays | VM | 6 | 9m32s |
| net | VM | 16 | 13m10s |
| launcher | VM | 1 | 2m42s |
| dumps | VM | 6 | 3m33s |
| browser | PC | 18 | 11m40s |
| ai_page | PC | 9 | 26m05s |
| shaders | PC | 4 | 1m36s |

The copy 29 s, the VM's run 42m37s, the results back 2m50s (one new kept folder). The long pole is
now the VM (part 5: the PC's browser + ai_page lane). Step 3.3 (browser and ai_page in two PC lanes)
was not tried: two headless Chromes need about 8 GB more and the PC had 16 GB free.

## F4: one shared queue, csp on the PC only

The second real full run (2026-11-23, main folder, `--full --farm`) had every area identical but
took **9h14m**, longer than the PC alone used to (about 6.5-7 h). The PC's lanes (browser 56 min,
ai_page 26, shaders 5) were done after about 1.5 h; then the PC sat idle for about 8 h while the VM
worked through F3's fixed lane plan (VM run 31,001 s, results back 2,149 s). That night the VM ran
3,410 min of case time (the sum of each area's `times.tsv`: fresh 900, yebis_sweep 673, render 576,
physics 328, replays 271, gui 267, dumps 262, net 98, csp 80, launcher 21, sound 13), about **3.9 ×**
the PC's time for the same areas: F3's ratios were about 1.8, but the server ran another heavy job
that night and eight lanes share 12 virtual CPUs.

### The queue

- **Words.** `farm_up` (after its checks and `prebuild.sh`) writes the queue: every farm area in
  shards (the existing `<area>.<i>` words) of about 15 min (`--full`) or 8 min (`--quick`) of
  measured case time, longest first. The times are the sum of `target/proofs/cases/<mode>/<area>.times`,
  which every run already rewrites, so the order improves itself; an area with no times yet goes
  first, whole. net stays whole (`net.sh` checks its goldens in one `check.sh` run, no `keep()`).
  The list is `run\queue\list` on the share and `target/proofs/farm/queue` here.
- **Taking.** Every PC lane, once its own areas are done, and one more PC lane from the start, take
  the next word nobody took; the VM runs eight lanes that do only that (`PROOF_QUEUE`, `PROOF_PULL`
  from `farm_run.sh`). GPU-heavy areas (browser, ai_page, shaders) and csp stay in the PC's own
  lanes as before; the queue fills the PC's free lanes after them.
- **Claimed exactly once.** A claim is `mkdir run\queue\claim\<word>`: the VM makes it on its own
  disk, the PC over SMB, and both end in one NTFS create on the VM, which succeeds for exactly one
  of them (no rename, no cached listing, no coordinator). The claim fails with "File exists" when
  taken; any other failure (the share gone) ends that lane's taking. Checked
  (`re/scratch/taskF4/claim_test.sh`): 4 PC lanes over SMB and 8 VM lanes raced for 400 words:
  400 of 400 taken, none twice, none missing (PC 4 × 25, VM 8 × 37-38).
- **No preemption.** A word started on one machine finishes there. Its results come back word by
  word (`farm_go`: folder, log, row of each word the VM finished); the PC merges the shards once
  both sides are done (`run_all.sh`'s `merge`, as for lanes before).
- **If the VM dies** (or stalls, or its share stops answering): the PC's lanes keep taking what is
  left while the share answers; when the farm half gives up (`FARM_STALL`, 10 min without the share,
  the task not starting) every word with no row here (the VM's unfinished ones and any never taken)
  runs on the PC from a local queue of only those, 4 lanes, with the same shard counts, its "where"
  `PC (VM: <reason>)`, and the table's last line says the farm stopped and why. Checked
  (`re/scratch/taskF4/vm_dies.sh`): every bash on the VM killed 90 s into its run, `FARM_STALL=2`:
  the VM's finished words (sound, python) came back, its unfinished `replays.2` ran on the PC, all
  IDENTICAL.
- **Copy failed / VM off / `--farm-fallback`.** A failure before the queue exists (no settings, no
  ssh, no share, an earlier farm run still running, `prebuild.sh`) stops or falls back area by area
  as in F3 (unchanged). A failure after it (the copy, the run folder, the task) only records why:
  the PC's lanes take the whole queue. Checked: `FARM_SYNC=bogus --quick --farm sound python
  launcher`: one PC lane took all three, 6/6 IDENTICAL, the last line "the farm stopped: the copy to
  the VM failed (no part bogus), the PC took the rest".
- **The table's "where"** says per area which machine ran which shards (`VM 1,2,5; PC 3,4`), and
  `PC (VM: <reason>)` for a word the PC ran because the VM could not; physics' note appears when any
  physics shard ran on the VM.
- **proof_tui**: a lane that takes from the queue is `lane @<k>` in `run.status`, its words so far in
  `lanes/<k>.lane` (the VM's mirrored too); proof_tui shows those words. Its time left counts only
  the words taken so far (shortcut, in the code).
- **Found on the way**: replays' `rustyac.exe replay at 0, 1, 8 workers` ran outside `keep()`, so each
  replays shard ran it and `merge` counted it twice (an ERROR the run-again pass turned back into
  IDENTICAL: the full run's replays row said 33 cases for 32). It now goes through `keep()` (one shard
  runs it) and is in `quick/replays.txt`.

### Per area: what a shard on the PC needs

Every case compares the game and the port on the machine it runs on, so a verdict never depends on
where it ran; what differs is what it proves and what it may have to record.

| area | on the PC | notes |
|---|---|---|
| physics | nothing | the PC proves MSVCR120.dll's maths, the VM `pure.rs`'s (F2); the note names it when any shard ran on the VM |
| fresh | the PC's kept recordings (FMA3 keys) | `archive_kept.sh` (dry run, 2026-11-23): the PC holds **26 of today's 217** keys, so a PC fresh shard mostly records the game again (about 1.7 × its time from kept results) and keeps what it recorded; priced in below, and it fills the PC's keys for later runs |
| dumps | the PC's kept dumps (FMA3 keys) | same as fresh, much smaller |
| net | nothing | off FMA3 the native ways take `pure.rs` (as physics) |
| render, gui, yebis_sweep, replays, sound, python, launcher | nothing | game and port live on the same machine |
| csp | PC only | Part 2 |

### csp on the PC only

The lead chat's decision (2026-11-22): CSP itself behaves differently on the VM's processor without
AVX, and the port's loader-based core cases don't run there yet. csp is off `areas.map`'s `@farm`
line, so it runs in the PC's own lanes; `farm_sync.sh` no longer has the `csp` part (CSP's build and
`tools/csp`), the repo part no longer sends `csp_oracle`'s `main.rs`, and `prebuild.sh` no longer
builds `csp_oracle`.

### `run_all.sh --quick --farm` (part 3)

Two farm runs from the worktree (`re/scratch/taskF4/quick_farm*.txt`) and a PC-only quick run
between them (`quick_pc.txt`, plus `quick_pc_dumps.txt`: dumps alone, after the `old_exe.sh` fix
below). **Every case has the same verdict in all three** (`verdicts_compare.txt`, case by case, all
14 areas); csp's one DIFFERENT is its known two-car row (docs/csp/core.md, C5i-c: car 1 differs from
step 1, also in Vanilla), on the PC in both runs.

| area | where (farm run 2) | cases | farm run 2 | PC alone |
|---|---|---|---|---|
| physics | PC | 9 | 0m48s | 1m01s |
| fresh | PC 1; VM 2,3,4 | 63 | 27m36s | 28m16s |
| csp | PC | 67 | 18m16s | 14m13s |
| sound | PC | 4 | 0m25s | 0m24s |
| render | VM 1; PC 2 | 18 | 22m30s | 8m55s |
| gui | VM 1,2,3 | 29 | 36m03s | 18m18s |
| python | PC | 1 | 0m13s | 0m12s |
| replays | VM | 6 | 22m51s | 5m10s |
| net | PC | 16 | 5m22s | 7m06s |
| browser | PC | 18 | 13m34s | 14m22s |
| ai_page | PC | 10 | 26m13s | 26m52s |
| shaders | PC | 4 | 1m01s | 1m51s |
| launcher | PC | 1 | 0m53s | 0m49s |
| dumps | PC | 6 | 1m58s | 2m12s |
| **wall clock** | | **253** | **0h41m** (42m08s with `prebuild.sh`) | **0h50m** (0h48m + dumps) |

(Times of a sharded area are its longest shard's.) Before F4 the last quick farm run took 0h45m (F3).
**The quick run's long pole is the PC's browser + ai_page lane** (40 min, as in F2 part 5); the
queue can't shorten that lane, so a quick run gains little more until browser and ai_page get two
lanes. The VM's run took 2,285 s (38 min), the copy 111 s (517 MB of `ac`), the results back 110 s
(75 new kept files of the VM).

- **Farm run 1** (the first after the master merge) took 1h40m, all verdicts the same: the PC's csp
  and browser lanes rebuilt `csp_oracle` and the wasm crates from cold (done 50 and 59 min into the
  run). The queue did its job around them: the VM took 25 of the 31 words (fresh 6 of 8, render 6 of
  7, gui 8 of 9), the PC's free lanes 6.
- **The shard sizes come from whichever machine ran a case last.** Run 1's times were the VM's (F3's
  run), so it made 31 words of about 8 VM-min; run 2's were the PC-only run's, so it made 12 words of
  about 8 PC-min, and a gui shard took 36 min on the VM. For a full run that would leave a long VM
  shard at the end, so the full target is 15 min (900 s), not 30; a `shortcut:` comment in `farm.sh`
  says to scale the times per machine if a VM shard still ends a run late.
- **A start cost on the VM, once per run**: in run 2 the first case of every VM shard took 12-18 min
  (all 8 lanes starting at once, after the copy put 517 MB into the VM's `ac`); in run 1 a shard
  started later spent 30-90 s before its first case. So a shard costs about a minute to start, and
  227 words (the next full run's queue) about 15 min of wall clock in all.

### The next full run (estimate)

From the 2026-11-23 run's case times (VM) and the PC's own area times (F3's list, the 2026-11-19 PC
full run; csp about 35 PC-min now, from its VM time and part 5's ratio): the farm areas are about
870 PC-min of work, plus about 100 min more for the share of fresh the PC records again (it holds
few keys); the PC's own lanes (browser 56, ai_page 26, shaders 5, csp about 35) about 120 PC-min.
The PC gives 4 lanes; the VM's eight lanes are worth 8 / 3.9 ≈ 2 PC lanes on a night like
2026-11-23, 8 / 1.8 ≈ 4.4 with the server idle (F3's ratios). On top: the start (`prebuild.sh`,
the copy, the VM's start cost: 10-25 min), the shards' own starts (about 15 min), the last shard
(up to one VM shard, 15-60 min), the results back (5-35 min: 2,149 s on 2026-11-23, nearly all the
VM's new kept recordings).

| | work (PC-min) | lanes (PC equivalents) | lanes busy | + the rest | **wall clock** |
|---|---|---|---|---|---|
| server busy (as 2026-11-23) | about 1,090 | 4 + 2.0 | about 3h00m | 45 min-2h15m | **about 3h45m-5h15m, likely about 4h** |
| server idle | about 1,060 | 4 + 4.4 | about 2h05m | 45 min-1h45m | **about 2h50m-3h50m, likely about 3h** |

Against 9h14m on 2026-11-23 and about 6.5-7 h for the PC alone. Runs after it get shorter again:
the fresh shards the PC records are kept, so later PC fresh shards run from kept results, and the
order and sizes come from the run before. The model assumes the PC keeps 4 lanes busy with heavy
areas next to Chrome (room() waits for 6 GB free before a heavy case; with 16-19 GB free that may
slow it).

### Also found

- **`old_exe.sh` could not build v1.1.5** (the dumps area's old release, also built by `prebuild.sh`
  for every `--farm` run): from v1.1.5 on rustyac.exe has the `embree` feature, and the tag's git
  worktree has no `ignored/embree`, so dumps "did not run" on the PC and the next `--farm` would have
  stopped at `prebuild.sh`. It now hands the build this checkout's built Embree
  (`RUSTYAC_EMBREE_LIB=target/embree-3.13.4-mt`, the same 3.13.4); v1.1.5's exe built in 2 min and
  dumps is 6/6 IDENTICAL.
- **Open: `archive_kept.sh --move` counts the VM's kept recordings as old.** Its dry run here lists
  186 old folders, 14.1 GB: it asks fresh.sh for today's keys on this PC's maths path (FMA3), so the
  VM's SSE2 recordings that `farm_go` brings back (930 files after 2026-11-23) look unused and
  would be archived. Harmless for the proofs (the VM keeps its own copy and the PC never reads
  them), but the PC then holds them for nothing until the next `--move`. Not changed here.
