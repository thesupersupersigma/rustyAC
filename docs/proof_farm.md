# Proof farm (Task F1 spike)

Can a Windows 10 virtual machine on the home server run the proof areas bit-exact, and how
fast? The VM has 12 virtual CPUs, 32 GB of memory and no GPU. Its host processor is an Intel
Xeon E5620 (Westmere, 2010), which has **no AVX and no FMA3**. The PC is an i9-9980HK, which
has both. Task F1 is a spike: it measures and judges, but it builds no scheduler (that is F2).

## Resume here

- Done: step 1 (part 1 below, from the code).
- Stopped before step 2 (2026-11-18): the VM is missing every prerequisite step 2 needs.
  - **Git Bash**: not installed (no `C:\Program Files\Git`, no `bash` on PATH).
  - **Python 3**: not installed (`python` and `python3` are only the Microsoft Store
    shortcuts, which print "Python was not found").
  - **Visual C++ 2013 x64 runtime**: not installed (no `msvcr120.dll` or `msvcp120.dll` in
    System32, and no `VisualStudio\12.0\VC\Runtimes\x64` registry key).
  - The VM also shows **2 logical processors** (2 sockets × 1 core), not the planned 12.
    That caps step 2's lane test at 2, and the farm's speed with it.
  - Windows 10 19045, 30 GB of memory free, 242 GB free on C:, OpenSSH works.
    `C:\rustyac-farm` does not exist yet.
- Next: once those three are installed and the VM has its 12 vCPUs, run step 2, the
  physics area on the VM.

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
