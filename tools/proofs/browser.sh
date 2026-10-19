#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The browser build's physics (Tasks 20w, 26b, 37): every recorded drive of oracle/game through
# replay_hash on the desktop (MSVCR120.dll) and built for wasm32-wasip1 under wasmtime (pure maths):
# the same hash of every step. BROWSER_DIGEST=1 adds the 16 shards of `math_proof digest` (all 2^32
# floats of each one-argument function, desktop against wasm; long).
area=browser
. "$(dirname "$0")/lib.sh"
wt=$repo/target/web-tools/wasmtime/bin/wasmtime.exe
need "$wt" "wasmtime (docs/port/web.md)"
need target/wasm-sysroot "the wasm sysroot (docs/port/web.md)"
cargo build --release --locked --quiet -p rustyac-game --bin replay_hash || exit 2
CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="--sysroot $repo/target/wasm-sysroot" \
    cargo build --release --locked --quiet -p rustyac-game --bin replay_hash --target wasm32-wasip1 || exit 2
docs="$USERPROFILE/Documents/Assetto Corsa"
for f in oracle/game/*.ryin; do
    d=$(target/release/replay_hash.exe "$f" 2>&1 | grep -o "hash.*")
    s=$(target/release/replay_hash.exe "$f" 2>&1 | grep -o "^[0-9]* steps")
    # a drive with the user's setup names it under Documents: seen read-only as /docs
    w=$(MSYS_NO_PATHCONV=1 "$wt" run --env AC_ROOT=/ac --dir "$repo_w::/repo" --dir "$ac::/ac" \
        --dir "$docs::/docs" target/wasm32-wasip1/release/replay_hash.wasm "/repo/$f" \
        --remap "$ac=/ac" --remap "$(cygpath -m "$docs" 2>/dev/null || echo "$docs")=/docs" --remap "$repo_w=/repo" 2>&1 | grep -o "hash.*")
    if [ -n "$d" ] && [ "$d" = "$w" ]; then verdict "$f" IDENTICAL "$s $d"; else verdict "$f" DIFFERENT "desktop [$d] wasm [$w]"; fi
done
if [ -n "$BROWSER_DIGEST" ]; then
    tool math_proof
    CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="--sysroot $repo/target/wasm-sysroot" \
        cargo build --release --locked --quiet --manifest-path tools/math_proof/Cargo.toml --target wasm32-wasip1 || exit 2
    for i in $(seq 0 15); do
        a=$(tools/math_proof/target/release/math_proof.exe digest --stride 16 --offset "$i" 2>&1 | tail -n 1)
        b=$(MSYS_NO_PATHCONV=1 "$wt" run tools/math_proof/target/wasm32-wasip1/release/math_proof.wasm digest --stride 16 --offset "$i" 2>&1 | tail -n 1)
        [ -n "$a" ] && [ "$a" = "$b" ] && verdict "digest shard $i" IDENTICAL "$a" || verdict "digest shard $i" DIFFERENT "desktop [$a] wasm [$b]"
    done
fi
summary
