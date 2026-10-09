# rustyAC in the browser: a WASM preview (Task 20w)

## Resume here

State: **Step 1 done** (pure-Rust maths, proven). Steps 2 to 4, the report and the version are open.

- Done and committed:
  - `crates/rustyac-math/src/pure.rs` + `pure_tables.rs`: MSVCR120's `sinf cosf tanf expf asinf acosf atanf
    atan2f powf` and the double `sin` in plain Rust (the FMA3 path the game runs)
  - `RUSTYAC_MATH=pure` selects it; a wasm build always uses it; the desktop default is unchanged
  - `tools/math_proof` (`all`, `fma3-off`, `one <fn>`, `parse <folders>`, `digest`): the proof against the
    real DLL; results in git-ignored `oracle/math/results.md`
  - `crates/rustyac-math/tests/pure.rs`: sample against the DLL + recorded hashes (the wasm build runs it)
- Next: Step 2 (physics on `wasm32-unknown-unknown` / `wasm32-wasip1`, replay proof under WASI)
- Notes for whoever resumes:
  - this PC has Rust 1.97 from the stand-alone installer, **no rustup**: the wasm `rust-std` has to be
    unpacked into a folder inside the repo and given to rustc with `--sysroot` (see "How to build")
  - scratch: `re/scratch/task20w/` (`dlldis.py` disassembles MSVCR120 exports into `asm/`,
    `recon_r*.md` are three read-only maps of the crates)

(The report below is filled in as the steps finish.)
