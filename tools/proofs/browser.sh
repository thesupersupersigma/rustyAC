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
CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="--sysroot $repo_w/target/wasm-sysroot" \
    cargo build --release --locked --quiet -p rustyac-game --bin replay_hash --target wasm32-wasip1 || exit 2
docs="$USERPROFILE/Documents/Assetto Corsa"
# conditions_spa_setup_launch names its setup under the deleted re/scratch/task15: the kept copy is in re/gold/fresh
t15=$repo_w/re/scratch/task15
for f in oracle/game/*.ryin; do
    keep "$f" || continue  # quick keys: oracle/game/<drive>.ryin
    r=$(target/release/replay_hash.exe "$f" --remap "$t15=$repo_w/re/gold/fresh" 2>&1)
    d=$(echo "$r" | grep -o "hash.*")
    s=$(echo "$r" | grep -o "^[0-9]* steps")
    # a drive with the user's setup names it under Documents: seen read-only as /docs
    w=$(MSYS_NO_PATHCONV=1 "$wt" run --env AC_ROOT=/ac --dir "$repo_w::/repo" --dir "$repo_w::." --dir "$ac::/ac" \
        --dir "$docs::/docs" target/wasm32-wasip1/release/replay_hash.wasm "/repo/$f" \
        --remap "$ac=/ac" --remap "$t15=/repo/re/gold/fresh" --remap "$(cygpath -m "$docs" 2>/dev/null || echo "$docs")=/docs" --remap "$repo_w=/repo" 2>&1 | grep -o "hash.*")
    if [ -n "$d" ] && [ "$d" = "$w" ]; then verdict "$f" IDENTICAL "$s $d"; else verdict "$f" DIFFERENT "desktop [$d] wasm [$w]"; fi
done
if [ -n "$BROWSER_DIGEST" ]; then
    tool math_proof
    CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="--sysroot $repo_w/target/wasm-sysroot" \
        cargo build --release --locked --quiet --manifest-path tools/math_proof/Cargo.toml --target wasm32-wasip1 || exit 2
    for i in $(seq 0 15); do
        a=$(tools/math_proof/target/release/math_proof.exe digest --stride 16 --offset "$i" 2>&1 | tail -n 1)
        b=$(MSYS_NO_PATHCONV=1 "$wt" run tools/math_proof/target/wasm32-wasip1/release/math_proof.wasm digest --stride 16 --offset "$i" 2>&1 | tail -n 1)
        [ -n "$a" ] && [ "$a" = "$b" ] && verdict "digest shard $i" IDENTICAL "$a" || verdict "digest shard $i" DIFFERENT "desktop [$a] wasm [$b]"
    done
fi

# the panel's on-demand servers (Task 39): the page under /play/app/ as on rustyac.com, the router's
# list at /net/x/servers.json (ondemand_servers.json); the theme lists them, ?join takes the
# listed server's car and track and its own gateway address, an unlisted ?join joins nothing
need "$gold/web/pack/manifest.json" "the test pack"
python web/build.py > "$out/build.txt" 2>&1 || { verdict "page build" DIFFERENT "$(tail -n 1 "$out/build.txt")"; summary; exit; }
python web/serve.py --port 8139 --pack "$gold/web/pack" > "$out/serve.txt" 2>&1 &
sp=$!
node tools/proofs/ondemand_site.mjs 8140 8139 tools/proofs/ondemand_servers.json > "$out/site.txt" 2>&1 &
fp=$!
trap 'kill $sp $fp 2>/dev/null' EXIT
sleep 5
page() { node web/check.mjs --url "http://127.0.0.1:8140/play/app/?$1" $2 --script tools/proofs/ondemand_page.js --profile "$out/chrome_profile" --port 9339 --timeout 600 2> "$out/page.err" | node -e 'let t="";process.stdin.on("data",(d)=>(t+=d)).on("end",()=>{const r=JSON.parse(t||"{}");console.log(JSON.stringify(r.script??r.error??null))})'; }
r=$(page "launcher=1" --launcher)
echo "$r" | grep -qF '{"name":"Proof Laguna","track":"ks_laguna_seca","cars":["bmw_z4_gt3"],"pass":true}' && verdict "theme lists the on-demand server" IDENTICAL "$r" || verdict "theme lists the on-demand server" DIFFERENT "$r"
r=$(page "join=x-proof-laguna&name=proof39" --menu)
echo "$r" | grep -qF '"server":"x-proof-laguna"' && echo "$r" | grep -qF 'no connection to ws://127.0.0.1:8140/net/x/proof-laguna' && verdict "?join of a listed on-demand server" IDENTICAL "$r" || verdict "?join of a listed on-demand server" DIFFERENT "$r"
r=$(page "join=x-not-running&name=proof39" --menu)
echo "$r" | grep -qF 'x-not-running is not running now' && ! echo "$r" | grep -qF 'connecting' && verdict "?join of an unlisted server" IDENTICAL "$r" || verdict "?join of an unlisted server" DIFFERENT "$r"
kill $sp $fp 2>/dev/null
rm -rf "$out/chrome_profile"
summary
