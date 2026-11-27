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
# docs/flags.md names what the page reads from its address and what web_pack accepts, and nothing else
r=$(python tools/proofs/url_params.py 2>&1 | tr -d '\r'; exit "${PIPESTATUS[0]}") && verdict "docs/flags.md, the page's address parameters" IDENTICAL "$r" ||
    verdict "docs/flags.md, the page's address parameters" DIFFERENT "the lists differ: $(echo "$r" | head -n 4 | tr '\n' ' ')"
cargo test --release --locked --quiet --manifest-path tools/web_pack/Cargo.toml usage_and_docs > "$out/web_pack_test.txt" 2>&1 && verdict "docs/flags.md, web_pack's options" IDENTICAL ||
    verdict "docs/flags.md, web_pack's options" DIFFERENT "the lists differ: $(grep -m1 -A2 'assertion' "$out/web_pack_test.txt" | tr '\n' ' ')"
build replay_hash cargo build --release --locked --quiet -p rustyac-game --bin replay_hash
CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="--sysroot $repo_w/target/wasm-sysroot" \
    cargo build --release --locked --quiet -p rustyac-game --bin replay_hash --target wasm32-wasip1 || exit 2
# conditions_spa_setup_launch names its setup under the deleted re/scratch/task15: the kept copy is in re/gold/fresh
t15=$repo_w/re/scratch/task15
# a checkout made by tools/worktree/make_web.ps1 has oracle/, re/ and cardata/ as junctions, which
# wasmtime does not follow out of its folders ("Operation not permitted"): their own folders are
# given as well, under /repo and under . (the test cars are read as ./cardata/<car>)
links=()
for d in oracle re cardata; do [ -L "$d" ] && links+=(--dir "$(cd "$d" && pwd -W)::/repo/$d" --dir "$(cd "$d" && pwd -W)::$d"); done
for f in oracle/game/*.ryin; do
    keep "$f" || continue  # quick keys: oracle/game/<drive>.ryin
    g=$(drive "$f")  # a drive naming the player's setup: a copy naming the kept one in re/gold/docs
    r=$(target/release/replay_hash.exe "$g" --remap "$t15=$repo_w/re/gold/fresh" 2>&1)
    d=$(echo "$r" | grep -o "hash.*")
    s=$(echo "$r" | grep -o "^[0-9]* steps")
    w=$(MSYS_NO_PATHCONV=1 "$wt" run --env AC_ROOT=/ac --dir "$repo_w::/repo" "${links[@]}" --dir "$repo_w::." --dir "$ac::/ac" \
        target/wasm32-wasip1/release/replay_hash.wasm "/repo/$g" \
        --remap "$ac=/ac" --remap "$t15=/repo/re/gold/fresh" --remap "$repo_w=/repo" 2>&1 | grep -o "hash.*")
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
# partial imports (Task W5): made-up folders of the install's own files at /parts/ (serve.py --mount,
# read in place), and one zip and one copied car (the file list) in $out
acm=$(cygpath -m "$ac")
mounts=()
for m in content_root/content/cars/ks_mazda_miata=cars/ks_mazda_miata content_root/content/tracks/magione=tracks/magione     cars_root/cars/abarth500=cars/abarth500 cars_root/tracks/drift=tracks/drift one_car/bmw_1m=cars/bmw_1m     dlc/cars/ks_mazda_mx5_cup=cars/ks_mazda_mx5_cup renamed/tracks/w5_magione=tracks/magione; do
    mounts+=(--mount "/parts/${m%%=*}/=$acm/content/${m#*=}")
done
rm -rf "$out/parts"
mkdir -p "$out/parts/zips" "$out/parts/copied/cars"
python - "$acm/content/cars/ks_toyota_gt86" "$out/parts/zips/ks_toyota_gt86.zip" <<'PY'
import os, sys, zipfile
src, dst = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(dst, 'w', zipfile.ZIP_DEFLATED) as z:
    for d, _, files in os.walk(src):
        for f in files:
            z.write(os.path.join(d, f), 'ks_toyota_gt86/' + os.path.relpath(os.path.join(d, f), src).replace(os.sep, '/'))
PY
cp -r "$ac/content/cars/abarth500" "$out/parts/copied/cars/"
mounts+=(--mount "/parts/zips/=$(cygpath -m "$out/parts/zips")")
MSYS2_ARG_CONV_EXCL=/parts python web/serve.py --port 8139 --pack "$(cygpath -m "$gold/web/pack")" --ac "$acm" "${mounts[@]}" > "$out/serve.txt" 2>&1 &
sp=$!
node tools/proofs/ondemand_site.mjs 8140 8139 tools/proofs/ondemand_servers.json > "$out/site.txt" 2>&1 &
fp=$!
trap 'kill $sp $fp 2>/dev/null; proof_unlock' EXIT
serving $sp "$out/serve.txt"
# a profile folder per page: a browser of the page before can still hold its folder for a while
page() { node web/check.mjs --url "http://127.0.0.1:8140/play/app/?$1" $2 --script tools/proofs/ondemand_page.js --profile "$out/chrome_profile_$(echo "$1" | cksum | cut -d" " -f1)" --port 9339 --timeout 600 2> "$out/page.err" | node -e 'let t="";process.stdin.on("data",(d)=>(t+=d)).on("end",()=>{const r=JSON.parse(t||"{}");console.log(JSON.stringify(r.script??r.error??null))})'; }
r=$(page "launcher=1" --launcher)
echo "$r" | grep -qF '{"name":"Proof Laguna","track":"ks_laguna_seca","cars":["bmw_z4_gt3"],"pass":true}' && verdict "theme lists the on-demand server" IDENTICAL "$r" || verdict "theme lists the on-demand server" DIFFERENT "$r"
r=$(page "join=x-proof-laguna&name=proof39" --menu)
echo "$r" | grep -qF '"server":"x-proof-laguna"' && echo "$r" | grep -qF 'no connection to ws://127.0.0.1:8140/net/x/proof-laguna' && verdict "?join of a listed on-demand server" IDENTICAL "$r" || verdict "?join of a listed on-demand server" DIFFERENT "$r"
r=$(page "join=x-not-running&name=proof39" --menu)
echo "$r" | grep -qF 'x-not-running is not running now' && ! echo "$r" | grep -qF 'connecting' && verdict "?join of an unlisted server" IDENTICAL "$r" || verdict "?join of an unlisted server" DIFFERENT "$r"
# Full mode on a track it cannot draw (Task W2): montreal's CSP layout names stPerPixelNM_UVflow,
# which the install's system/shaders/win lacks; the debug view draws and the page's banner says why
if [ -d "$ac/content/tracks/montreal" ]; then
    r=$(node web/check.mjs --url "http://127.0.0.1:8139/?testfolder=/ac-test/&go=1&car=ks_ferrari_f2004&track=montreal&spawn=pit&autodrive=1&backend=full" --seconds 5     --script tools/proofs/fallback_page.js --profile "$out/chrome_profile_fallback" --port 9340 --timeout 900 --flags "--enable-features=Vulkan" 2> "$out/fallback.err" |
        node -e 'let t="";process.stdin.on("data",(d)=>(t+=d)).on("end",()=>{const r=JSON.parse(t||"{}");console.log(JSON.stringify(r.script??r.error??null))})')
    echo "$r" | grep -qF '"shown":true' && echo "$r" | grep -qF 'Full mode could not start' && echo "$r" | grep -qF 'stPerPixelNM_UVflow' && echo "$r" | grep -qF '"same":true' &&
        verdict "Full mode's fallback banner (montreal)" IDENTICAL "$r" || verdict "Full mode's fallback banner (montreal)" DIFFERENT "$r"
else skip "Full mode's fallback banner (montreal)" "the montreal mod is not installed"; fi
# the render scale (Task W3): Full mode at 50 % from the start, and 100 % turned to 50 % mid-drive;
# each screenshot one whole picture (scale_check.py: no black quarter, no app bar off the edge)
scale_case() {
    node web/check.mjs --url "http://127.0.0.1:8139/?testfolder=/ac-test/&go=1&car=ks_ferrari_f2004&track=ks_red_bull_ring&spawn=pit&autodrive=1&backend=full&graphics=high,SCALE=$2" \
        --seconds 8 $3 --screenshot "$out/scale_$2.png" --profile "$out/chrome_profile_scale" --port 9342 --timeout 900 --flags "--enable-features=Vulkan" > "$out/scale_$2.json" 2>&1
    r=$(python tools/proofs/scale_check.py "$out/scale_$2.png" 2>&1 | tr -d '\r')
    case $r in ok*) verdict "$1" IDENTICAL "$r" ;; *) verdict "$1" DIFFERENT "$r" ;; esac
}
scale_case "Full mode at render scale 50 %" 50
scale_case "Full mode's render scale 100 % to 50 % mid-drive" 100 "--script tools/proofs/scale_page.js"
# the way back to AC's launcher (Task W1): 3 drives with sound left by AC's Exit (FMOD's late mix
# once hung the page there), then 3 reloads from the launcher and 3 from a drive
python tools/proofs/roundtrip_flow.py "$out/roundtrip_flow.json" 3 3 ks_ferrari_f2004@spa
node web/check.mjs --url "http://127.0.0.1:8139/?" --flow "$out/roundtrip_flow.json" --profile "$out/chrome_profile_roundtrip" --port 9341 --timeout 1800 \
    --flags "--enable-features=Vulkan" > "$out/roundtrip.json" 2> "$out/roundtrip.err"
while IFS='|' read -r name v what; do verdict "$name" "$v" "$what"; done < <(node tools/proofs/roundtrip_verdicts.mjs < "$out/roundtrip.json" | tr -d '\r')
# partial imports (Task W5), over the test pack and over the install: both layouts, one car, a zip,
# the file list, the theme's lists, drives, and a DLC's model without the DLC
imp() { node web/check.mjs --url "http://127.0.0.1:8139/?$1" "${@:3}" --profile "$out/chrome_profile_imports" --port 9343 --timeout 900 > "$out/imports_$2.json" 2> "$out/imports_$2.err"
    node -e 'const r=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")||"{}");const s=r.script||{};console.log(JSON.stringify({state:r.state,error:r.error,kmh:r.kmh,cars:s.cars,tracks:s.tracks,theme:s.theme,picked:r.picked&&r.picked.line}))' "$out/imports_$2.json"; }
four="testimport=/parts/content_root/,/parts/cars_root/,/parts/one_car/,/parts/zips/ks_toyota_gt86.zip"
r=$(imp "launcher=0&$four" menu --menu --script tools/proofs/imports_page.js)
echo "$r" | grep -qF '"cars":["ks_ferrari_f2004","bmw_z4_gt3","bmw_m3_e30","abarth500","bmw_1m","ks_mazda_miata","ks_toyota_gt86"]' && echo "$r" | grep -qF '"tracks":["spa|","ks_laguna_seca|","ks_drag|drag400","drift|","magione|"]' &&
    verdict "partial imports over the pack (content/, cars and tracks/, one car, a zip)" IDENTICAL "$r" || verdict "partial imports over the pack (content/, cars and tracks/, one car, a zip)" DIFFERENT "$r"
r=$(imp "launcher=1&$four" theme --launcher --script tools/proofs/imports_page.js)
echo "$r" | grep -qF '"theme":["ks_mazda_miata","abarth500","bmw_1m","ks_toyota_gt86","magione","drift","bmw_z4_gt3"]' && verdict "the theme lists the imports" IDENTICAL "$r" || verdict "the theme lists the imports" DIFFERENT "$r"
r=$(imp "launcher=0&testimport=/parts/zips/ks_toyota_gt86.zip&go=1&car=ks_toyota_gt86&track=ks_laguna_seca&spawn=pit&autodrive=1" zipdrive --seconds 8)
echo "$r" | grep -qF '"state":"driving"' && verdict "a drive in a car from a zip" IDENTICAL "$r" || verdict "a drive in a car from a zip" DIFFERENT "$r"
r=$(imp "launcher=0&testfolder=/ac-test/&testimport=/parts/renamed/&go=1&car=bmw_1m&track=w5_magione&spawn=pit&autodrive=1" folderdrive --seconds 8)
echo "$r" | grep -qF '"state":"driving"' && verdict "a drive on a track added over the install" IDENTICAL "$r" || verdict "a drive on a track added over the install" DIFFERENT "$r"
r=$(imp "launcher=0&testimport=/parts/dlc/&go=1&car=ks_mazda_mx5_cup&track=ks_laguna_seca&spawn=pit&autodrive=1" dlc --seconds 3)
echo "$r" | grep -qF '"error":"this car needs the Japanese Pack DLC"' && verdict "an added car whose model needs a DLC" IDENTICAL "$r" || verdict "an added car whose model needs a DLC" DIFFERENT "$r"
r=$(imp "launcher=0" list --menu --pick-folder "$(cygpath -w "$out/parts/copied")" --script tools/proofs/imports_page.js)
echo "$r" | grep -qF '"picked":"preview pack and copied: 4 cars, 3 tracks and layouts"' && verdict "the file list of a folder of cars" IDENTICAL "$r" || verdict "the file list of a folder of cars" DIFFERENT "$r"
rm -rf "$out/parts"
kill $sp $fp 2>/dev/null
rm -rf "$out"/chrome_profile*
summary
