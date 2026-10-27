#!/bin/sh
# Task 38j step 4: two headless Chromes join one server in Full mode, all on 127.0.0.1:
# Kunos' acServer (a copy of dist-server, Laguna Seca, no lobby), the gateway, web/serve.py
# with the test pack. Each page reports the drivers it sees and the other car's place, then
# leaves (window.rustyac.leave); the server and the gateway say who left.
#   sh tools/web_checks/task38j/mp.sh   (needs web/build.py's build and target/release/rustyac-gateway.exe)
cd "$(dirname "$0")/../../.." || exit 1
here=re/scratch/task38j
run=$here/mp
rm -rf "$run" && mkdir -p "$run"
port=9640 http=8194 gw=9641

srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http '
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-38j"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }

(cd "$srv" && exec timeout 600 ./acServer.exe > "../mp/server.log" 2>&1) &
server_job=$!
target/release/rustyac-gateway.exe --name gw --listen 127.0.0.1:$gw --server 127.0.0.1:$port > "$run/gateway.log" 2>&1 &
gw_job=$!
py web/serve.py --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --port 8188 --quiet > "$run/serve.log" 2>&1 &
sv=$!
sleep 4

# at the end of each visit: what it saw, then the leave
cat > "$run/end.js" <<'EOF'
const online = window.rustyac.online;
const before = { state: window.rustyac.state, notes: (window.rustyac.notes || '').split('\n').slice(0, 2), full: !(window.rustyac.notes || '').includes('Full mode could not start'), drivers: online ? online.drivers.map((d) => ({ me: d.me, name: d.name, car: d.car, laps: d.laps })) : null, log: window.rustyac.log.slice(-3) };
window.rustyac.leave();
await new Promise((r) => setTimeout(r, 100)); // (leaving reloads the page 150 ms later)
return { before, after: { state: window.rustyac.state, online: !!window.rustyac.online, mp: document.getElementById('mp-status').textContent } };
EOF
client() { # client <name> <cdp port> <seconds>: in the background
    node web/check.mjs --url "http://127.0.0.1:8188/?testfolder=/ac-test/&join=address&server=ws://127.0.0.1:$gw/net/laguna&servercars=ks_ferrari_f2004,bmw_z4_gt3&servertrack=ks_laguna_seca&autodrive=1&spawn=pit&mpcar=ks_ferrari_f2004&name=$1&backend=full" \
        --seconds "$3" --profile "$here/prof_mp_$1" --port "$2" --timeout 900 --flags "--use-webgpu-adapter=swiftshader" --script "$run/end.js" > "$run/$1.json" 2> "$run/$1.err" &
}
client a 9361 150; a=$!
sleep 15
client b 9362 120; b=$!
wait $a $b
sleep 3
kill $gw_job $sv $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
wait 2>/dev/null
rm -rf "$here/prof_mp_"* "$srv"

{
    grep -E "Clean exit|disconnected|NEW PICKUP|KICK|Removing|LAP " "$run/server.log" | head -20
    grep -E " left after |joined|stopped" "$run/gateway.log" | head -10
    for f in "$run"/?.json; do
        py - "$f" <<'PY'
import json, sys, os
f = sys.argv[1]
try:
    r = json.load(open(f, encoding='utf-8'))
except Exception as e:
    print(os.path.basename(f), 'no report', e); sys.exit()
print(os.path.basename(f)[0], '| state', r.get('state'), '| error', r.get('error'), '| others seen', r.get('others_seen'), '| others', json.dumps(r.get('others'))[:300])
print('    console:', [m for m in r.get('console') or [] if 'error' in m.lower()][:10])
print('   ', json.dumps(r.get('script'))[:900])
PY
    done
} > "$run/summary.txt"
cat "$run/summary.txt"
