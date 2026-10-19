#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# AI drivers in the page (Tasks 37c, 38): Task 37c's Quick Race (seed 28005, the desktop's tape in
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
kill $sp 2>/dev/null
rm -rf "$out/chrome_profile"
summary
