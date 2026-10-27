#!/bin/sh
# Task 39 step 0 end to end, all on 127.0.0.1, with 38e's stand-ins (fake_docker.mjs, edge.mjs):
# the panel starts "rustyAC Montreal" (Corvette C7R at Montreal, a made-up password); the page
# sits under /play/app/ and the router at /net/x/* on one address (tools/proofs/ondemand_site.mjs),
# as on rustyac.com. Checked: the router's list, the theme's online page, ?join with no car named
# (the listed car and track), ?join with a wrong password, an unlisted ?join, and the page of
# v0.38.9 (git's web/app.js) on the same ?join. The browser pack is re/scratch/task39/webpack.
#   sh tools/web_checks/task39/e2e.sh
cd "$(dirname "$0")/../../.." || exit 1
here=re/scratch/task39
old=re/scratch/task38e
out=$here/e2e
rm -rf "$out" && mkdir -p "$out"
abs() { (cd "$1" && pwd -W); }
docker_port=9670 certs=9678 edge=9679 panel=9680 router=9681 page=8187 site=8188 oldpage=8189 oldsite=8190
pw=made-up-pw-39

rm -rf $here/site_old && cp -r dist-web/site $here/site_old && git show v0.38.9:web/app.js > $here/site_old/app.js

node tools/web_checks/task38e/fake_docker.mjs $docker_port $out/docker > $out/docker.log 2>&1 &
pids=$!
node tools/web_checks/task38e/edge.mjs $certs $edge $panel > $out/edge.log 2>&1 &
pids="$pids $!"
mkdir -p $out/data
PANEL_LOCAL_ONLY=1 GATEWAY_BIND=127.0.0.1 PANEL_DOCKER_URL=http://127.0.0.1:$docker_port \
  PANEL_DATA=$out/data PANEL_DATA_DIR="$(abs $out/data)" PANEL_PACK=dist-server AC_PACK_DIR="$(abs dist-server)" \
  CF_ACCESS_TEAM_DOMAIN=example-team.cloudflareaccess.com CF_ACCESS_AUD=made-up-aud-38e \
  CF_ACCESS_CERTS_URL=http://127.0.0.1:$certs/cdn-cgi/access/certs \
  GATEWAY_ORIGINS="http://127.0.0.1:$site,http://127.0.0.1:$oldsite" PANEL_SITE_URL=http://127.0.0.1:$site/play/app/ PANEL_GAME_HOST=127.0.0.1 PANEL_POLL_SECONDS=5 \
  node server/panel/src/main.ts > $out/panel.log 2>&1 &
pids="$pids $!"
py web/serve.py --site dist-web/site --pack $here/webpack --port $page --quiet > $out/serve.log 2>&1 &
pids="$pids $!"
py web/serve.py --site $here/site_old --pack $here/webpack --port $oldpage --quiet > $out/serve_old.log 2>&1 &
pids="$pids $!"
node tools/proofs/ondemand_site.mjs $site $page $router > $out/site.log 2>&1 &
pids="$pids $!"
node tools/proofs/ondemand_site.mjs $oldsite $oldpage $router > $out/site_old.log 2>&1 &
pids="$pids $!"
trap 'kill $pids 2>/dev/null' EXIT
sleep 4

say() { echo "$*" | tee -a $out/result.txt; }
api() { curl -s -H 'Content-Type: application/json' "$@"; }
state() { curl -s --max-time 5 http://127.0.0.1:$edge/api/state; }
report() { node -e "const r=require('./$1');console.log(JSON.stringify({state:r.state,error:r.error,script:r.script}))"; }

say "== the preset: rustyAC Montreal, 4 Corvette C7R, a password"
node -e "
const p = { id: 'rustyac-montreal', name: 'rustyAC Montreal', track: 'montreal', layout: '',
  cars: [{ model: 'ks_corvette_c7r', count: 4, skins: [], random: false }],
  practice: { minutes: 30, open: 1 }, qualify: null, race: null, time: '14:00',
  weather: { graphics: '3_clear', ambient: 20, road: 26, variationAmbient: 1, variationRoad: 1, windMin: 0, windMax: 3, windDirection: 0, windVariation: 15 },
  dynamicTrack: { start: 96, randomness: 1, transfer: 80, lapGain: 30 }, password: '$pw', maxPlayers: 4, pickup: true, idleMinutes: 3, rules: {} };
process.stdout.write(JSON.stringify(p));" > $out/preset.json
saved=$(api -X POST --data-binary @$out/preset.json http://127.0.0.1:$edge/api/presets)
say "saved: $saved"
echo "$saved" | grep -q '"ok"' || { say "not saved: stop"; exit 1; }
say "start: $(api -X POST -d '{}' http://127.0.0.1:$edge/api/start/rustyac-montreal)"
waited=0
until state | grep -qF '"status":"running"'; do
  waited=$((waited + 2)); [ $waited -gt 120 ] && { say "never running"; exit 1; }
  sleep 2
done
sleep 3
say "server_cfg.ini: $(grep -E '^(TRACK|CARS|PASSWORD)=' $out/data/run/rustyac-montreal/server_cfg.ini | tr '\n' ' ')"
say "router list (through the site): $(curl -s http://127.0.0.1:$site/net/x/servers.json)"
say "the panel's running entry: $(state | node -e "let t='';process.stdin.on('data',d=>t+=d).on('end',()=>{const r=JSON.parse(t).running[0];console.log(JSON.stringify({id:r.id,status:r.status,password:r.password}))})")"

say "== AC's launcher in the page (/play/app/): its online page"
node web/check.mjs --url "http://127.0.0.1:$site/play/app/?launcher=1" --launcher --profile $here/chrome-l --port 9352 --timeout 300 \
  --script tools/proofs/ondemand_page.js --screenshot $here/page_launcher.jpg > $out/launcher.json 2> $out/launcher.err
say "launcher: $(report $out/launcher.json)"

join() {  # join <site port> <query> <chrome port> [--menu]
  node web/check.mjs --url "http://127.0.0.1:$1/play/app/?$2" $4 --seconds 20 --profile $here/chrome-$3 --port $3 --timeout 450 \
    --script tools/web_checks/task39/joined.js > $out/$3.json 2> $out/$3.err
  say "$(report $out/$3.json)"
}
say "== ?join=x-rustyac-montreal, no car named, the password"
join $site "join=x-rustyac-montreal&password=$pw&autodrive=1&spawn=pit&name=e2e39" 9353
say "== the same, a wrong password"
join $site "join=x-rustyac-montreal&password=not-it&name=e2e39b" 9354 --menu
say "== an unlisted server"
join $site "join=x-not-running&name=e2e39c" 9355 --menu
say "== v0.38.9's page on the same ?join"
join $oldsite "join=x-rustyac-montreal&password=$pw&autodrive=1&spawn=pit&name=e2e39d" 9356 --menu

say "== stop"
say "stop: $(api -X POST -d '{}' http://127.0.0.1:$edge/api/stop/rustyac-montreal)"
sleep 3
say "router list after: $(curl -s http://127.0.0.1:$site/net/x/servers.json)"
rm -rf $here/chrome-* $here/site_old
