# Proof farm (Task F1 spike)

Can a Windows 10 virtual machine on the home server run the proof areas bit-exact, and how
fast? The VM has 12 virtual CPUs, 32 GB of memory and no GPU. Its host processor is an Intel
Xeon E5620 (Westmere, 2010), which has **no AVX and no FMA3**. The PC is an i9-9980HK, which
has both. Task F1 is a spike: it measures and judges, but it builds no scheduler (that is F2).

## Resume here

- Done: step 1 (part 1), step 2 (part 2: the physics area on the VM, bit-exact; the maths
  checks).
- Done: step 3 (part 2b, on paper) and step 4 (part 2c: GO with conditions). F1 is finished; F2 next.

## Part 1: what the processor changes (from the code)

### The msvcr120 switch

`MSVCR120.dll` (12.0.21005.1) holds two bodies for `sinf`, `cosf`, `tanf`, `expf`, `logf`,
`powf` and `sin` (double). Each one starts with `cmp [__use_fma3_lib], 0`. The runtime sets
that flag once, at load time, when the processor has FMA3 and the OS saves the AVX state.
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

Everything sits in `C:\rustyac-farm\` on the VM. The script is `re/scratch/taskF1/copy.sh`
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

Lanes: n copies of the farm repo (`re/scratch/taskF1/stage/vm_lanes.sh`), n quick physics runs
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

`re/scratch/taskF1/stage/vm_math.sh`; results in `re/scratch/taskF1/vm_math.txt`.

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
