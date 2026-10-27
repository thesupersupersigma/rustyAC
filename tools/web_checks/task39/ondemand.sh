#!/bin/sh
area=browser_ondemand
. tools/proofs/lib.sh
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
