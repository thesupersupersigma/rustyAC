#!/bin/sh
# Task 42b step 2: the panel's form in a headless Chrome, all on 127.0.0.1 (38e's stand-ins):
# a new preset's sessions, then the same form made race-only (the note shows).
#   sh tools/web_checks/task42b/s2/panel.sh
cd "$(dirname "$0")/../../../.." || exit 1
here=re/scratch/task42b/s2
old=re/scratch/task38e
out=$here/out
rm -rf "$out" && mkdir -p "$out/data"
abs() { (cd "$1" && pwd -W); }
docker_port=9670 certs=9678 edge=9679 panel=9680
node tools/web_checks/task38e/fake_docker.mjs $docker_port $out/docker > $out/docker.log 2>&1 &
pids=$!
node tools/web_checks/task38e/edge.mjs $certs $edge $panel > $out/edge.log 2>&1 &
pids="$pids $!"
PANEL_LOCAL_ONLY=1 GATEWAY_BIND=127.0.0.1 PANEL_DOCKER_URL=http://127.0.0.1:$docker_port \
  PANEL_DATA=$out/data PANEL_DATA_DIR="$(abs $out/data)" PANEL_PACK=dist-server AC_PACK_DIR="$(abs dist-server)" \
  CF_ACCESS_TEAM_DOMAIN=example-team.cloudflareaccess.com CF_ACCESS_AUD=made-up-aud-38e \
  CF_ACCESS_CERTS_URL=http://127.0.0.1:$certs/cdn-cgi/access/certs \
  PANEL_SITE_URL=http://127.0.0.1:8188/ PANEL_GAME_HOST=127.0.0.1 PANEL_POLL_SECONDS=5 \
  node server/panel/src/main.ts > $out/panel.log 2>&1 &
pids="$pids $!"
trap 'kill $pids 2>/dev/null' EXIT
sleep 4
show="const f = (n) => document.querySelector('[name=' + n + ']').value; document.title = JSON.stringify({ practice: f('practice'), qualify: f('qualify'), raceLaps: f('raceLaps'), note: !document.getElementById('raceOnly').hidden }); document.getElementById('raceOnly').scrollIntoView({ block: 'center' });"
node tools/web_checks/task38e/shot.mjs "http://127.0.0.1:$edge/" $out/form_new.jpg 3 "document.getElementById('new').click(); $show"
node tools/web_checks/task38e/shot.mjs "http://127.0.0.1:$edge/" $out/form_race_only.jpg 3 "document.getElementById('new').click(); const p = document.querySelector('[name=practice]'); p.value = '0'; p.dispatchEvent(new Event('input', { bubbles: true })); $show"
