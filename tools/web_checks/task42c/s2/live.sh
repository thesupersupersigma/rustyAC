#!/bin/bash
# Task 42c step 2 (from 42b step 1): a race-only server (as the panel's race-only preset: one race, no practice),
# all on 127.0.0.1: Kunos' acServer (a copy of dist-server, Laguna Seca, no lobby), the gateway,
# web/serve.py. Desktop "a" races from the start (autodrive); while the race runs, the page
# "b" (Full mode, real keys) and the desktop "c" (autodrive) join. What each sees, then the
# race's end (results, back to the pits) and the server's next race.
#   sh tools/web_checks/task42c/s2/live.sh [laps]
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
here=re/scratch/task42c/s2
run=$here/run
laps=${1:-1}
rm -rf "$run" && mkdir -p "$run"
port=9650 http=8196 gw=9651 site=8189

srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
# race only: [PRACTICE] and [QUALIFY] left out; a short race, short end screens
awk -v port=$port -v http=$http -v laps=$laps '
    /^\[/ { skip = ($0 == "[PRACTICE]" || $0 == "[QUALIFY]") }
    skip { next }
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-42c"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    /^LAPS=/ { print "LAPS=" laps; next }
    /^WAIT_TIME=/ { print "WAIT_TIME=20"; next }
    /^RACE_OVER_TIME=/ { print "RACE_OVER_TIME=20"; next }
    /^RESULT_SCREEN_TIME=/ { print "RESULT_SCREEN_TIME=20"; next }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }
grep -q "^\[PRACTICE\]" "$srv/cfg/server_cfg.ini" && { echo "practice left in" >&2; exit 1; }

(cd "$srv" && exec timeout 900 ./acServer.exe > "../run/server.log" 2>&1) &
server_job=$!
target/release/rustyac-gateway.exe --name gw --listen 127.0.0.1:$gw --server 127.0.0.1:$port > "$run/gateway.log" 2>&1 &
gw_job=$!
py web/serve.py --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --port $site --quiet > "$run/serve.log" 2>&1 &
sv=$!
sleep 4
mkdir -p "$run/empty_cfg" "$run/empty_docs"
desk() { # desk <name> <seconds> [args]: a desktop client
    target/release/rustyac.exe --join 127.0.0.1:$port --car "${CAR:-ks_ferrari_f2004}" --name "$1" --no-steam --cfg-dir "$run/empty_cfg" --documents "$run/empty_docs" --duration "$2" "${@:3}" > "$run/desk_$1.log" 2>&1
}
CAR=${ACAR:-ks_ferrari_f2004} desk a ${ASECS:-330} --autodrive --pit-key drive@${DRIVE_AT:-262} &
a=$!
sleep ${JOIN_AT:-8}
grep -E "RACE|session|Session" "$run/server.log" | tail -3 > "$run/at_join.txt"
python tools/web_checks/task42c/s2/mkflow.py "$run/b_flow.json" "$run"
node web/check.mjs --url "http://127.0.0.1:$site/?testfolder=/ac-test/&launcher=0&join=address&server=ws://127.0.0.1:$gw/net/laguna&servercars=ks_ferrari_f2004,bmw_z4_gt3&servertrack=ks_laguna_seca&spawn=pit&mpcar=ks_ferrari_f2004&name=b&backend=full&autodrive=1" \
    --flow "$run/b_flow.json" --profile "$here/prof_b" --port 9372 --timeout 600 --flags "${FLAGS:---enable-features=Vulkan}" > "$run/b.json" 2> "$run/b.err" &
b=$!
wait $b $a
kill $gw_job $sv $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
wait 2>/dev/null
rm -rf "$here/prof_b" "$srv" "$run/empty_cfg" "$run/empty_docs"
{
    echo "server at the join: $(tr '\n' ' ' < "$run/at_join.txt")"
    grep -E "NEW PICKUP|RACE|session|LAP |disconnected|Clean exit" "$run/server.log" | head -30
    for d in a; do echo "== desktop $d"; grep -E "^online: (the|a new)|The race|the car after|laps:" "$run/desk_$d.log"; done
    echo "== page b"
    py -c "
import json
r = json.load(open('$run/b.json'))
for s in r['flow']:
    print({k: str(v)[:300] for k, v in s.items() if k != 'cdp' or 'failed' in s})
print('console', r.get('console'))"
} > "$run/summary.txt"
cat "$run/summary.txt"
