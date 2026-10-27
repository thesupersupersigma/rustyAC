#!/bin/sh
# Task 38e: clippy every way (CI's lists + the Task 38 extras) and the --locked builds.
cd "$(dirname "$0")/../../.." || exit 1
O=re/scratch/task38e/checks
REPO=$(pwd -W)
export CARGO_BUILD_JOBS=4
export CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="--sysroot $REPO/target/wasm-sysroot"
run() { name=$1; shift; "$@" > $O/$name.log 2>&1; echo "$name exit=$? warnings=$(grep -c '^warning\|^error' $O/$name.log)"; }
run clippy_native cargo clippy --workspace --locked --all-targets -- -D warnings
run clippy_wasm_unknown cargo clippy -p rustyac-web -p rustyac-net --lib --target wasm32-unknown-unknown --locked -- -D warnings
run clippy_wasip1 cargo clippy -p rustyac-math -p rustyac-ode -p rustyac-content -p rustyac-physics -p rustyac-net -p rustyac-game --target wasm32-wasip1 --locked -- -D warnings
run clippy_render_wgpu cargo clippy --release --locked --all-targets -p rustyac-render --features wgpu -- -D warnings
run clippy_web_wasm cargo clippy --release --locked --all-targets -p rustyac-web -p rustyac-dxbc --target wasm32-unknown-unknown -- -D warnings
run build_release cargo build --release --locked
for m in tools/*/Cargo.toml; do
  run "build_$(basename $(dirname $m))" cargo build --release --locked --manifest-path $m
done
