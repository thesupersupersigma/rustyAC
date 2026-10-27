#!/bin/sh
# Task 38e end to end, all on 127.0.0.1: the panel (node server/panel/src/main.ts) against
# the Docker stand-in (fake_docker.mjs: the acServer copy and the repo's gateway as local
# processes), behind the Access stand-in (edge.mjs); the page (web/serve.py with this task's
# app.js) lists the started server from the router and a headless Chrome joins it through
# the router; the server then stops itself after its (shortened) idle minutes.
#   sh tools/web_checks/task38e/e2e.sh
cd "$(dirname "$0")/../../.." || exit 1
here=re/scratch/task38e
out=$here/e2e
rm -rf "$out" && mkdir -p "$out"
abs() { (cd "$1" && pwd -W); }
docker_port=9670 certs=9678 edge=9679 panel=9680 router=9681 site=8188

# the page: v0.38.2's wasm and site files (task38d) with this task's app.js / index.html
rm -rf $here/site && cp -r re/scratch/task38d/site_new $here/site && cp web/app.js web/index.html $here/site/

node tools/web_checks/task38e/fake_docker.mjs $docker_port $out/docker > $out/docker.log 2>&1 &
pids=$!
node tools/web_checks/task38e/edge.mjs $certs $edge $panel > $out/edge.log 2>&1 &
pids="$pids $!"
mkdir -p $out/data
PANEL_LOCAL_ONLY=1 GATEWAY_BIND=127.0.0.1 PANEL_DOCKER_URL=http://127.0.0.1:$docker_port \
  PANEL_DATA=$out/data PANEL_DATA_DIR="$(abs $out/data)" PANEL_PACK=dist-server AC_PACK_DIR="$(abs dist-server)" \
  CF_ACCESS_TEAM_DOMAIN=example-team.cloudflareaccess.com CF_ACCESS_AUD=made-up-aud-38e \
  CF_ACCESS_CERTS_URL=http://127.0.0.1:$certs/cdn-cgi/access/certs \
  GATEWAY_ORIGINS=http://127.0.0.1:$site PANEL_SITE_URL=http://127.0.0.1:$site/ PANEL_GAME_HOST=127.0.0.1 PANEL_POLL_SECONDS=5 \
  node server/panel/src/main.ts > $out/panel.log 2>&1 &
pids="$pids $!"
py web/serve.py --site $here/site --pack re/scratch/task38d/pack --port $site --quiet > $out/serve.log 2>&1 &
pids="$pids $!"
trap 'kill $pids 2>/dev/null' EXIT
sleep 4

say() { echo "$*" | tee -a $out/result.txt; }
api() { curl -s -H 'Content-Type: application/json' "$@"; }
state() { curl -s --max-time 5 http://127.0.0.1:$edge/api/state; }

say "== Access"
say "no token: $(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:$panel/)"
say "made-up token: $(curl -s -o /dev/null -w '%{http_code}' -H 'Cf-Access-Jwt-Assertion: eyJhbGciOiJSUzI1NiIsImtpZCI6InRlc3Qta2V5LTM4ZSJ9.eyJhdWQiOiJ4In0.c2ln' http://127.0.0.1:$panel/api/state)"
say "through the edge: $(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:$edge/)"
say "a form post (not JSON): $(curl -s -o /dev/null -w '%{http_code}' -d 'x=1' http://127.0.0.1:$edge/api/copy/laguna_seca)"

say "== preset"
say "copy of laguna_seca: $(api -X POST -d '{}' http://127.0.0.1:$edge/api/copy/laguna_seca)"
# a shortened idle time (2 minutes) and a password-less preset
node -e "
const p = JSON.parse(require('fs').readFileSync('$out/data/presets/laguna_seca-x.json', 'utf8'));
p.idleMinutes = 2;
process.stdout.write(JSON.stringify(p));" > $out/preset.json
say "saved with 2 idle minutes: $(api -X POST --data-binary @$out/preset.json http://127.0.0.1:$edge/api/presets)"
say "a preset with a car not in the pack: $(node -e "const p=require('./$out/preset.json');p.id='bad';p.cars[0].model='ks_mazda_mx5_cup';process.stdout.write(JSON.stringify(p))" | api -X POST --data-binary @- http://127.0.0.1:$edge/api/presets)"
say "server_cfg.ini of the start = laguna_seca's but its ports: see cfg.diff"

say "== start"
say "start: $(api -X POST -d '{}' http://127.0.0.1:$edge/api/start/laguna_seca-x)"
started=$(date +%s)
diff dist-server/cfg/laguna_seca/server_cfg.ini $out/data/run/laguna_seca-x/server_cfg.ini > $out/cfg.diff
diff dist-server/cfg/laguna_seca/entry_list.ini $out/data/run/laguna_seca-x/entry_list.ini > $out/entry.diff
say "entry_list.ini differs from laguna_seca's in $(grep -c '^[<>] [^;]' $out/entry.diff) non-comment lines"
sleep 8
say "router list: $(curl -s http://127.0.0.1:$router/net/x/servers.json)"
say "router, unknown server: $(curl -s -o /dev/null -w '%{http_code}' -H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' http://127.0.0.1:$router/net/x/nope)"
say "router, a fixed server's path: $(curl -s -o /dev/null -w '%{http_code}' -H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' http://127.0.0.1:$router/net/spa)"
node tools/web_checks/task38e/shot.mjs "http://127.0.0.1:$edge/" $here/panel_running.jpg 3 | tee -a $out/result.txt

say "== a browser joins through the router"
( while :; do echo "$(($(date +%s) - started)) $(state | node -e "let t='';process.stdin.on('data',d=>t+=d).on('end',()=>{const s=JSON.parse(t);console.log(JSON.stringify(s.running.map(r=>[r.id,r.status,r.players,r.stopsInSeconds])))})")"; sleep 5; done ) > $out/poll.txt 2>&1 &
poller=$!
pids="$pids $poller"
node web/check.mjs --url "http://127.0.0.1:$site/?join=x-laguna_seca-x&ondemand=http://127.0.0.1:$router&autodrive=1&spawn=pit&mpcar=ks_ferrari_f2004&name=panel38e" \
  --seconds 40 --profile $here/chrome-a --port 9351 --timeout 300 --script tools/web_checks/task38d/mpdiag.js --screenshot $here/page_joined.jpg > $out/client.json 2> $out/client.err
left=$(date +%s)
say "client: $(node -e "const r=require('./$out/client.json');console.log(JSON.stringify({state:r.state,error:r.error,online:r.online&&{session:r.online.session,cars:r.online.cars},mp:r.script&&r.script.mp,kmh:r.kmh}))")"
node tools/web_checks/task38e/shot.mjs "http://127.0.0.1:$edge/" $here/panel_player.jpg 2 >/dev/null
curl -s http://127.0.0.1:$edge/api/logs/laguna_seca-x > $out/logs.json

say "== the idle stop (2 minutes after the last player)"
while state | grep -qF '"running":[{'; do
  [ $(($(date +%s) - left)) -gt 400 ] && { say "still running 400 s after the client left"; break; }
  sleep 5
done
say "gone $(($(date +%s) - left)) s after the client left ($(($(date +%s) - started)) s after the start)"
say "router list after: $(curl -s http://127.0.0.1:$router/net/x/servers.json)"
say "containers left in the stand-in: $(curl -s 'http://127.0.0.1:'$docker_port'/containers/json?all=1' | tr -d '\n')"
say "run folder left: $(ls $out/data/run 2>/dev/null | tr '\n' ' ')"
kill $poller
grep -h "stopped\|started\|refused" $out/panel.log | tee -a $out/result.txt
