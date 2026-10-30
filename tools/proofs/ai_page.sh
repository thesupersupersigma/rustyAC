#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# AI drivers and the game modes in the page (Tasks 37c, 38, 45): Task 37c's Quick Race (seed 28005, the desktop's tape in
# re/gold/ai_page) three ways: the desktop's replay, native race_proof, and the page in headless
# Chrome (its own frame loop): the same race_out.json as Task 38's page run, byte for byte.
# Needs the test pack in re/gold/web/pack (tools/web_pack with web/preview.toml, see the README).
area=ai_page
. "$(dirname "$0")/lib.sh"
q=$gold/ai_page
need "$q/full.ryin" "Task 37c's Quick Race tape"
need "$gold/web/pack/manifest.json" "the test pack"
cargo build --release --locked --quiet -p rustyac-game --bin rustyac || exit 2
target/release/rustyac.exe --replay "$q/full.ryin" --headless --state-hash --physics-threads 8 > "$out/desktop.txt" 2>&1
grep -q '^state hash 7aea29db0a574b43: 2 cars, 93774 steps' "$out/desktop.txt" && verdict "desktop replay" IDENTICAL "$(head -n 1 "$out/desktop.txt")" || verdict "desktop replay" DIFFERENT "$(tail -n 1 "$out/desktop.txt")"
cargo run --release --locked --quiet -p rustyac-web --example race_proof -- --race "$q/race.ini" --assists "$q/assists.ini" --seed 28005 --tape "$q/full.ryin" --out "$out/native" > "$out/native.txt" 2>&1
cmp -s "$out/native/race_out.json" "$q/race_out.json" && verdict "native race_proof race_out.json" IDENTICAL || verdict "native race_proof race_out.json" DIFFERENT

python web/build.py > "$out/build.txt" 2>&1 || { verdict "page build" DIFFERENT "$(tail -n 1 "$out/build.txt")"; summary; exit; }
python web/serve.py --port 8137 --pack "$gold/web/pack" > "$out/serve.txt" 2>&1 &
sp=$!
trap 'kill $sp 2>/dev/null' EXIT
sleep 5
js() { node -e 'process.stdout.write(JSON.stringify(require("fs").readFileSync(process.argv[1],"utf8")))' "$1"; }
{
    printf 'for (let i = 0; i < 600 && !(window.rustyac && window.rustyac.frameProof && window.rustyac.launcher); i++) await new Promise((r) => setTimeout(r, 100));\n'
    printf 'return await window.rustyac.frameProof(%s, %s, "%s", 0, 0);\n' "$(js "$q/race.ini")" "$(js "$q/assists.ini")" "$(base64 -w0 "$q/full.ryin")"
} > "$out/page.js"
node web/check.mjs --url "http://127.0.0.1:8137/?seed=28005" --launcher --script "$out/page.js" --profile "$out/chrome_profile" --port 9338 --timeout 3600 > "$out/page.json" 2> "$out/page.err"
node -e '
const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
const s = r.script || {};
require("fs").writeFileSync(process.argv[2], s.race_out || "");
console.log(`step ${s.step} chain ${s.chain} ${(s.fps||0).toFixed(1)} fps, error ${r.error}`);
' "$out/page.json" "$out/page_race_out.json" > "$out/page.txt" 2>&1
cmp -s "$out/page_race_out.json" "$q/race_out.json" && verdict "page frameProof race_out.json" IDENTICAL "$(cat "$out/page.txt")" \
    || verdict "page frameProof race_out.json" DIFFERENT "$(cat "$out/page.txt")"
# Task 45: Time Attack, Drift and Drag Race from race.ini. The desktop drives each (autodrive,
# recorded), then race_proof (the page's setup and code, native) and, for the pack's Spa, the page
# in Chrome replay the tape: race_out.json (the mode's extras too) byte for byte as the desktop's.
# (The drag runs at ks_drag, which the test pack does not have: native only.)
rm -rf "$out/chrome_profile"
m=$out/modes
mkdir -p "$m"
printf '[RACE]\nTRACK=spa\nCONFIG_TRACK=\nMODEL=ks_ferrari_f2004\nCARS=1\n[CAR_0]\nMODEL=ks_ferrari_f2004\nSKIN=\n[SESSION_0]\nNAME=Time Attack\nTYPE=5\nSPAWN_SET=START\n' > "$m/ta.ini"
sed -e 's/=Time Attack/=Drift/' -e 's/^TYPE=5/TYPE=6/' "$m/ta.ini" > "$m/drift.ini"
printf '[RACE]\nTRACK=ks_drag\nCONFIG_TRACK=drag400\nMODEL=bmw_m3_e30\nCARS=2\nAI_LEVEL=100\n[CAR_0]\nMODEL=bmw_m3_e30\nSKIN=\n[CAR_1]\nMODEL=bmw_m3_e30\nSKIN=\nAI_LEVEL=100\n[SESSION_0]\nNAME=Drag Race\nTYPE=7\nSPAWN_SET=START\nMATCHES=3\n' > "$m/drag.ini"
# (both read an empty Documents folder, as the page's own: the player's gameplay.ini changes Time Attack's budget)
mkdir -p "$m/home"
home=$(cygpath -w "$m/home")
mode() { # mode() <name> <seed> <seconds> <rustyac.exe options>: the desktop's drive and race_proof's
    local name=$1 seed=$2 secs=$3; shift 3
    mkdir -p "$m/$name" && cp "$m/$name.ini" "$m/$name/race.ini"
    # (drift without traction or stability control: the autodriver slides now and then)
    case $name in drift) sed -e 's/^STABILITY_CONTROL=.*/STABILITY_CONTROL=0/' -e 's/^TRACTION_CONTROL=.*/TRACTION_CONTROL=0/' "$q/assists.ini" ;; *) cat "$q/assists.ini" ;; esac > "$m/$name/assists.ini"
    USERPROFILE="$home" target/release/rustyac.exe --headless --no-shm --no-audio --race-ini "$m/$name/race.ini" --autodrive --duration "$secs" --race-seed "$seed" --record "$m/$name/drive.ryin" --out-dir "$m/$name/desktop" "$@" > "$m/$name/desktop.txt" 2>&1
    USERPROFILE="$home" cargo run --release --locked --quiet -p rustyac-web --example race_proof -- --race "$m/$name/race.ini" --assists "$m/$name/assists.ini" --seed "$seed" --tape "$m/$name/drive.ryin" --out "$m/$name/native" > "$m/$name/native.txt" 2>&1
    cmp -s "$m/$name/native/race_out.json" "$m/$name/desktop/race_out.json" && grep -q "^setup: the recorded drive's" "$m/$name/native.txt" \
        && verdict "$name: native race_proof race_out.json" IDENTICAL "$(node -e 'console.log(JSON.stringify(JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")).extras[0]))' "$m/$name/desktop/race_out.json")" \
        || verdict "$name: native race_proof race_out.json" DIFFERENT "$(tail -n 1 "$m/$name/native.txt")"
}
# (the page's seed n is the desktop's --race-seed n --ai-seed n-28000; a drive alone has no AI seed: 28000)
mode ta 28000 200 --track spa --car ks_ferrari_f2004
mode drift 28000 90 --track spa --car ks_ferrari_f2004
mode drag 28005 120 --track ks_drag --layout drag400 --car bmw_m3_e30 --ai-seed 5
for name in ta drift; do
    {
        printf 'for (let i = 0; i < 600 && !(window.rustyac && window.rustyac.frameProof && window.rustyac.launcher); i++) await new Promise((r) => setTimeout(r, 100));\n'
        printf 'return await window.rustyac.frameProof(%s, %s, "%s", 0, 0);\n' "$(js "$m/$name/race.ini")" "$(js "$m/$name/assists.ini")" "$(base64 -w0 "$m/$name/drive.ryin")"
    } > "$m/$name/page.js"
    node web/check.mjs --url "http://127.0.0.1:8137/?seed=28000" --launcher --script "$m/$name/page.js" --profile "$out/chrome_profile" --port 9338 --timeout 3600 > "$m/$name/page.json" 2> "$m/$name/page.err"
    node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); require("fs").writeFileSync(process.argv[2], (r.script || {}).race_out || ""); console.log(`step ${(r.script||{}).step}, error ${r.error}`);' "$m/$name/page.json" "$m/$name/page_race_out.json" > "$m/$name/page.txt" 2>&1
    cmp -s "$m/$name/page_race_out.json" "$m/$name/desktop/race_out.json" && verdict "$name: page frameProof race_out.json" IDENTICAL "$(cat "$m/$name/page.txt")" \
        || verdict "$name: page frameProof race_out.json" DIFFERENT "$(cat "$m/$name/page.txt")"
    rm -rf "$out/chrome_profile"
done
kill $sp 2>/dev/null
rm -rf "$out/chrome_profile"
summary
