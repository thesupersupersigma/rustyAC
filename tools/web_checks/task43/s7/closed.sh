#!/bin/bash
# Task 43 step 7: who refuses "Session is closed to new connections", and what the page says.
# All on 127.0.0.1: Kunos' acServer (a copy of dist-server, Laguna Seca, race only, the given
# [RACE] IS_OPEN), the gateway, web/serve.py. Desktop "a" joins at once and drives; once the
# race has started, desktop "c" and the page "b" try to join.
#   sh tools/web_checks/task43/s7/closed.sh [is_open]     (2: closed once the race starts; 1: open)
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
open=${1:-2}
here=re/scratch/task43/s7
run=$here/run_$open
rm -rf "$run" && mkdir -p "$run"
port=9660 http=8197 gw=9661 site=8190

srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http -v open=$open '
    /^\[/ { skip = ($0 == "[PRACTICE]" || $0 == "[QUALIFY]"); race = ($0 == "[RACE]") }
    skip { next }
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-43"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    /^LAPS=/ { print "LAPS=5"; next }
    /^WAIT_TIME=/ { print "WAIT_TIME=20"; next }
    race && /^IS_OPEN=/ { print "IS_OPEN=" open; next }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }
grep -A8 "^\[RACE\]" "$srv/cfg/server_cfg.ini" | grep "^IS_OPEN=$open" > "$run/is_open.txt" || { echo "IS_OPEN not set" >&2; exit 1; }

(cd "$srv" && exec timeout 300 ./acServer.exe > "../run_$open/server.log" 2>&1) &
server_job=$!
target/release/rustyac-gateway.exe --name gw --listen 127.0.0.1:$gw --server 127.0.0.1:$port > "$run/gateway.log" 2>&1 &
gw_job=$!
py web/serve.py --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --port $site --quiet > "$run/serve.log" 2>&1 &
sv=$!
sleep 4
mkdir -p "$run/empty_cfg" "$run/empty_docs"
desk() { # desk <name> <seconds> [args]: a desktop client
    target/release/rustyac.exe --join 127.0.0.1:$port --car ks_ferrari_f2004 --name "$1" --no-steam --cfg-dir "$run/empty_cfg" --documents "$run/empty_docs" --duration "$2" "${@:3}" > "$run/desk_$1.log" 2>&1
}
desk a 120 --autodrive &
a=$!
# the race starts WAIT_TIME after the server's start
sleep 45
grep -E "RACE|session|Session" "$run/server.log" | tail -3 > "$run/at_join.txt"
desk c 20 --autodrive
echo '[{"until": "error"}, {"js": "document.getElementById(\"refusal\").innerText"}, {"shot": "'"$run"'/b_refusal.png"}]' > "$run/b_flow_closed.json"
echo '[{"until": "driving"}, {"wait": 3}, {"js": "window.rustyac.state"}]' > "$run/b_flow_open.json"
[ "$open" = 2 ] && flow=closed || flow=open
node web/check.mjs --url "http://127.0.0.1:$site/?testfolder=/ac-test/&launcher=0&join=address&server=ws://127.0.0.1:$gw/net/laguna&servercars=ks_ferrari_f2004,bmw_z4_gt3&servertrack=ks_laguna_seca&mpcar=ks_ferrari_f2004&name=b" \
    --flow "$run/b_flow_$flow.json" --profile "$here/prof_b" --port 9373 --timeout 180 > "$run/b.json" 2> "$run/b.err"
kill $a $gw_job $sv $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
wait 2>/dev/null
rm -rf "$here/prof_b" "$srv" "$run/empty_cfg" "$run/empty_docs"
{
    echo "$(cat "$run/is_open.txt") ; server when c and b joined: $(tr '\n' ' ' < "$run/at_join.txt")"
    grep -iE "closed|NEW PICKUP|REJECT|session" "$run/server.log" | head -20
    for d in a c; do echo "== desktop $d"; grep -iE "join|refus|closed|ERROR|slot" "$run/desk_$d.log" | head -5; done
    echo "== page b"
    py -c "
import json
r = json.load(open('$run/b.json'))
print('error', r.get('error') if 'error' in r else (r.get('page') or {}).get('error'))
for s in r.get('flow', []):
    print({k: str(v)[:400] for k, v in s.items()})"
} > "$run/summary.txt"
cat "$run/summary.txt"
