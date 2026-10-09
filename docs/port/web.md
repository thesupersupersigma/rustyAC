# rustyAC in the browser: a WASM preview (Task 20w)

## Resume here

State: **Steps 1 and 2 done and proven.** Step 3 (the web app) is in progress, Step 4, the report and the
version are open.

- Done and committed:
  - Step 1: `crates/rustyac-math/src/pure.rs` + `pure_tables.rs` (MSVCR120's maths in plain Rust, the FMA3
    path), `RUSTYAC_MATH=pure`, `tools/math_proof` (`all`, `fma3-off`, `one`, `rcpps`, `parse`, `digest`),
    `crates/rustyac-math/tests/pure.rs`
  - Step 2: `rustyac_content::vfs` (disk, or files in memory when mounted), `install::set_ac_root`, Intel's
    `rcpps` as a table for non-x86 (`rustyac-ode/src/rcp_table.rs`), the Windows-only parts of `rustyac-game`
    behind `cfg(windows)` (`src/desktop.rs` is the old `main.rs`, `input/live.rs` the live devices),
    `replay_hash` (`crates/rustyac-game/src/bin/`), `catalog::check_without_models`
  - the proof: `python re/scratch/task20w/replay_proof.py` (160 drives, 1,787,777 steps, all identical;
    table in `re/scratch/task20w/proof/results_final.md`)
  - `crates/rustyac-web`: `content`, `session`, `driver`, `model`, `bc`, `pack` (tested on the desktop:
    `cargo test --release -p rustyac-web`), `gpu.rs` + `shader.wgsl` + `web.rs` (compile for wasm, not yet
    run in a browser)
- Next: `web/` (page, `build.py`, `serve.py`), `tools/web_pack`, then Step 4 (headless Chromium), CI,
  report, version 0.20.1 and the tag
- Notes for whoever resumes:
  - this PC has Rust 1.97 from the stand-alone installer, **no rustup**: the wasm `rust-std` is unpacked in
    `target/wasm-sysroot` and given to rustc with
    `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="--sysroot <repo>/target/wasm-sysroot"` (same for
    `WASM32_WASIP1`)
  - pinned tools are downloaded into `target/web-tools/` (wasm-bindgen 0.2.129, binaryen version_123);
    wasmtime 25.0.2 for the WASI proof is in `re/scratch/task20w/tools/`
  - never start `chrome.exe` without `--headless=new --user-data-dir=<a folder under re/scratch>`: it joins
    the user's running browser otherwise
  - the code of `rustyac-physics` / `rustyac-game` is **not** rustfmt-formatted (long lines): do not run
    `cargo fmt` on them; `rustyac-math` and `tools/math_proof` are
  - scratch: `re/scratch/task20w/` (`dlldis.py` disassembles MSVCR120 exports into `asm/`, `rcp/` measured
    rcpps, `recon_r*.md` are three read-only maps of the crates)

(The report below is filled in as the steps finish.)
