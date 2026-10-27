#!/bin/sh
# Task 38g end to end, all on 127.0.0.1, with 38e's stand-ins (fake_docker.mjs, edge.mjs,
# shot.mjs): the panel on the --all pack (dist-server with index.json) saves a preset on a car
# and a track the old two-server pack did not have (Toyota GT86 at Magione, random skins);
# AC's launcher in the browser page lists it on its online page once it runs (the page opened
# before the start, not reloaded); a headless Chrome joins it through the router; it stops
# itself after its idle minutes. The browser pack is re/scratch/task38g/webpack (web_pack:
# ks_toyota_gt86 + magione).
#   sh tools/web_checks/task38g/e2e.sh
cd "$(dirname "$0")/../../.." || exit 1
here=re/scratch/task38g
old=re/scratch/task38e
out=$here/e2e
rm -rf "$out" && mkdir -p "$out"
abs() { (cd "$1" && pwd -W); }
docker_port=9670 certs=9678 edge=9679 panel=9680 router=9681 site=8188

rm -rf $here/site && cp -r re/scratch/task38d/site_new $here/site && cp web/*.js web/index.html $here/site/

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
# (not --quiet: lobby_list.js's "ready" request is read from this log)
py web/serve.py --site $here/site --pack $here/webpack --port $site > $out/serve.log 2>&1 &
pids="$pids $!"
trap 'kill $pids 2>/dev/null' EXIT
sleep 4

say() { echo "$*" | tee -a $out/result.txt; }
api() { curl -s -H 'Content-Type: application/json' "$@"; }
state() { curl -s --max-time 5 http://127.0.0.1:$edge/api/state; }

say "== Access and the page's files"
say "no token: $(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:$panel/)"
say "through the edge: $(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:$edge/)"
for f in /panel.js /form.js /favicon.svg /nope.js; do
  say "$f: $(curl -s -o /dev/null -w '%{http_code} %{content_type}' http://127.0.0.1:$edge$f)"
done
say "CSP: $(curl -s -D - -o /dev/null http://127.0.0.1:$edge/ | grep -i content-security-policy | tr -d '\r')"
say "inline scripts in the page: $(curl -s http://127.0.0.1:$edge/ | grep -c '<script>')"

say "== the pack"
state > $out/state0.json
say "$(node -e "const s=require('./$out/state0.json');console.log(s.pack.cars.length+' cars, '+s.pack.tracks.length+' tracks and layouts, '+s.pack.weather.length+' weathers; GT86: '+JSON.stringify(s.pack.cars.find(c=>c.model==='ks_toyota_gt86'))+'; Magione: '+JSON.stringify(s.pack.tracks.find(t=>t.track==='magione')))")"

say "== presets"
node -e "
const s = require('./$out/state0.json');
const gt86 = s.pack.cars.find((c) => c.model === 'ks_toyota_gt86');
const p = { id: 'gt86-magione', name: 'GT86 at Magione', track: 'magione', layout: '',
  cars: [{ model: 'ks_toyota_gt86', count: 6, skins: gt86.skins.slice(0, 4), random: true }],
  practice: { minutes: 20, open: 1 }, qualify: null, race: { laps: 0, minutes: 15, waitSeconds: 60, open: 1 }, time: '14:00',
  weather: { graphics: '4_mid_clear', ambient: 20, road: 6, variationAmbient: 1, variationRoad: 1, windMin: 0, windMax: 5, windDirection: 90, windVariation: 15 },
  dynamicTrack: { start: 96, randomness: 1, transfer: 80, lapGain: 30 }, password: '', maxPlayers: 6, pickup: true, idleMinutes: 2, rules: {} };
process.stdout.write(JSON.stringify(p));" > $out/preset.json
saved=$(api -X POST --data-binary @$out/preset.json http://127.0.0.1:$edge/api/presets)
say "GT86 at Magione, 6 cars, 4 skins at random: $(echo "$saved" | head -c 200)"
echo "$saved" | grep -q '"error"' && { say "not saved: stop"; exit 1; }
bad() { node -e "const p=require('./$out/preset.json');p.id='bad';$1;process.stdout.write(JSON.stringify(p))" | api -X POST --data-binary @- http://127.0.0.1:$edge/api/presets; }
say "a car not in the pack (MX-5 Cup, a DLC not owned): $(bad "p.cars[0].model='ks_mazda_mx5_cup'")"
say "19 cars on Magione's 18 pit boxes: $(bad "p.cars[0].count=19")"
say "a skin the car has not: $(bad "p.cars[0].skins=['not_a_skin']")"
say "a weather not in the pack: $(bad "p.weather.graphics='9_storm'")"

say "== AC's launcher in the page: its online page before and after the start"
node web/check.mjs --url "http://127.0.0.1:$site/?launcher=1&ondemand=http://127.0.0.1:$router" --launcher \
  --profile $here/chrome-l --port 9352 --timeout 300 --script tools/web_checks/task38g/lobby_list.js --screenshot $here/page_launcher.jpg > $out/launcher.json 2> $out/launcher.err &
lobby=$!
waited=0
until grep -q ready-38g $out/serve.log; do
  waited=$((waited + 2)); [ $waited -gt 300 ] && { say "the launcher page never got ready"; break; }
  sleep 2
done

say "== start"
say "start: $(api -X POST -d '{}' http://127.0.0.1:$edge/api/start/gt86-magione)"
started=$(date +%s)
( while :; do echo "$(($(date +%s) - started)) $(state | node -e "let t='';process.stdin.on('data',d=>t+=d).on('end',()=>{const s=JSON.parse(t);console.log(JSON.stringify(s.running.map(r=>[r.id,r.status,r.players,r.stopsInSeconds])))})")"; sleep 1; done ) > $out/poll.txt 2>&1 &
poller=$!
pids="$pids $poller"
cp $out/data/run/gt86-magione/server_cfg.ini $out/data/run/gt86-magione/entry_list.ini $out/ 2>/dev/null
say "server_cfg.ini: $(grep -E '^(TRACK|CONFIG_TRACK|CARS|MAX_CLIENTS|PICKUP_MODE_ENABLED|GRAPHICS)=' $out/server_cfg.ini | tr '\n' ' ')"
say "entry_list.ini: $(grep -E '^(MODEL|SKIN)=' $out/entry_list.ini | tr '\n' ' ')"
wait $lobby
say "launcher: $(node -e "const r=require('./$out/launcher.json');console.log(JSON.stringify({state:r.state,error:r.error,script:r.script}))")"
sleep 2
say "router list: $(curl -s http://127.0.0.1:$router/net/x/servers.json)"
node tools/web_checks/task38e/shot.mjs "http://127.0.0.1:$edge/" $here/panel_running.jpg 3 | tee -a $out/result.txt

say "== a browser joins through the router"
node web/check.mjs --url "http://127.0.0.1:$site/?join=x-gt86-magione&ondemand=http://127.0.0.1:$router&autodrive=1&spawn=pit&mpcar=ks_toyota_gt86&name=panel38g" \
  --seconds 40 --profile $here/chrome-a --port 9351 --timeout 300 --script tools/web_checks/task38d/mpdiag.js --screenshot $here/page_joined.jpg > $out/client.json 2> $out/client.err
left=$(date +%s)
say "client: $(node -e "const r=require('./$out/client.json');console.log(JSON.stringify({state:r.state,error:r.error,online:r.online&&{session:r.online.session,cars:r.online.cars},mp:r.script&&r.script.mp,kmh:r.kmh}))")"
curl -s http://127.0.0.1:$edge/api/logs/gt86-magione > $out/logs.json
say "acServer's skins: $(node -e "const l=require('./$out/logs.json');console.log(JSON.stringify(l).match(/SKIN[^\\\\]{0,60}/g)?.slice(0,6))" 2>/dev/null)"

say "== the new form (screenshots)"
edit="[...document.querySelectorAll('button')].filter((b) => b.textContent === 'Edit').at(-1).click(); document.getElementById('editor').scrollIntoView()"
node tools/web_checks/task38e/shot.mjs "http://127.0.0.1:$edge/" $here/form_edit.jpg 3 "$edit" | tee -a $out/result.txt
node tools/web_checks/task38e/shot.mjs "http://127.0.0.1:$edge/" $here/form_new.jpg 3 "document.getElementById('new').click(); document.querySelector('[name=presetName]').value='Spa GT3 night'; document.querySelector('[name=presetName]').dispatchEvent(new Event('input')); document.getElementById('carSearch').value='bmw'; document.getElementById('carSearch').dispatchEvent(new Event('input')); document.querySelector('details').open=true; document.getElementById('editor').scrollIntoView()" | tee -a $out/result.txt

say "== the idle stop (2 minutes after the last player)"
while state | grep -qF '"running":[{'; do
  [ $(($(date +%s) - left)) -gt 400 ] && { say "still running 400 s after the client left"; break; }
  sleep 5
done
say "gone $(($(date +%s) - left)) s after the client left ($(($(date +%s) - started)) s after the start)"
say "statuses while it started: $(head -8 $out/poll.txt | tr '\n' ' ')"
say "router list after: $(curl -s http://127.0.0.1:$router/net/x/servers.json)"
say "containers left in the stand-in: $(curl -s 'http://127.0.0.1:'$docker_port'/containers/json?all=1' | tr -d '\n')"
kill $poller
grep -h "stopped\|started\|refused" $out/panel.log | tee -a $out/result.txt
