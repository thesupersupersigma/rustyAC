#!/bin/bash
# Task 43b step 2c: the page joins a server through AC's launcher in it (Drive -> Online, the
# address bar's server, the Ferrari, Join), the race ends, the page leaves, and AC's launcher
# shows its results page from the page's race_out.json. All on 127.0.0.1 (from Task 42c's
# s2/live.sh): Kunos' acServer (a race-only copy of dist-server, Laguna Seca, LAPS laps, no
# lobby), the gateway, web/serve.py; desktop "a" (autodrive) finishes so the race ends. Two laps
# by default: a's first lap from the pits is often cut, and the theme's results page throws on
# a race with laps but no valid one (bestLaps empty: results.js reads bestLaps[0].car).
#   sh tools/web_checks/task43b/s2c/live.sh
cd "$(dirname "$0")/../../../.." || exit 1
here=re/scratch/task43b/s2c
run=$here/run
rm -rf "$run" && mkdir -p "$run"
port=9650 http=8196 gw=9651 site=8189

srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http -v laps="${LAPS:-2}" '
    /^\[/ { skip = ($0 == "[PRACTICE]" || $0 == "[QUALIFY]") }
    skip { next }
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-43b"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    /^LAPS=/ { print "LAPS=" laps; next }
    /^WAIT_TIME=/ { print "WAIT_TIME=40"; next }
    /^RACE_OVER_TIME=/ { print "RACE_OVER_TIME=20"; next }
    /^RESULT_SCREEN_TIME=/ { print "RESULT_SCREEN_TIME=20"; next }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }

(cd "$srv" && exec timeout 900 ./acServer.exe > "../run/server.log" 2>&1) &
server_job=$!
timeout 900 target/release/rustyac-gateway.exe --name gw --listen 127.0.0.1:$gw --server 127.0.0.1:$port > "$run/gateway.log" 2>&1 &
gw_job=$!
timeout 900 py web/serve.py --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --port $site --quiet > "$run/serve.log" 2>&1 &
sv=$!
sleep 4
mkdir -p "$run/empty_cfg" "$run/empty_docs"
timeout 900 target/release/rustyac.exe --join 127.0.0.1:$port --car ks_ferrari_f2004 --name a --no-steam --cfg-dir "$run/empty_cfg" --documents "$run/empty_docs" --out-dir "$(pwd -W)/$run/out_a" --duration ${ASECS:-420} --autodrive --pit-key drive@${DRIVE_AT:-262} > "$run/desk_a.log" 2>&1 &
a=$!
sleep 8
python tools/web_checks/task43b/s2c/mkflow.py "$run/b_flow.json" "$run" ks_ferrari_f2004
# (no ?join: AC's launcher in the page; the address bar's server is the one picked)
timeout 900 node web/check.mjs --url "http://127.0.0.1:$site/?testfolder=/ac-test/&server=ws://127.0.0.1:$gw/net/laguna&servercars=ks_ferrari_f2004,bmw_z4_gt3&servertrack=ks_laguna_seca&spawn=pit&name=b&backend=full&autodrive=1" \
    --flow "$run/b_flow.json" --profile "$here/prof_b" --port 9372 --timeout 600 --flags "${FLAGS:---enable-features=Vulkan}" > "$run/b.json" 2> "$run/b.err" &
b=$!
wait $b $a
kill $gw_job $sv $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
wait 2>/dev/null
rm -rf "$here/prof_b" "$srv" "$run/empty_cfg" "$run/empty_docs"
{
    grep -E "NEW PICKUP|RACE|LAP |disconnected|Clean exit" "$run/server.log" | head -30
    echo "== desktop a"; grep -E "^online: (the|a new)|The race|laps:|race_out" "$run/desk_a.log"
    echo "== page b"
    py -c "
import json
r = json.load(open('$run/b.json'))
for s in r['flow']:
    print({k: str(v)[:1500] for k, v in s.items()})
print('console', r.get('console'))"
} > "$run/summary.txt"
cat "$run/summary.txt"
