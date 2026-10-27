#!/bin/sh
# Multiplayer drop paths, old page (v0.38.2) against the fixed one, all on 127.0.0.1:
# Kunos' acServer (a copy of dist-server, Laguna Seca, made-up passwords, no lobby), the
# gateway, web/serve.py with the test pack, headless Chrome clients (web/check.mjs).
#   sh tools/web_checks/task38d/mp.sh old|new <case>
# cases:
#   cutjoin    B's download of the track breaks off (B knows the server's cars: servers.json)
#   cutlate    the same, but the page does not know the server's cars before the join
#   gwclose    the gateway goes away 30 s into the drive (what a closed tunnel does)
#   refused    two drive, a third asks with a wrong password (the 0 s visits of the incident)
#   freeze     B's page is frozen 60 s (a tab in the background, a sleeping Chromebook)
#   session    practice of 1 minute: the session changes while they drive
cd "$(dirname "$0")/../../.." || exit 1
side=$1 case=$2
here=re/scratch/task38d
run=$here/mp_${side}_$case
rm -rf "$run" && mkdir -p "$run"
[ "$side" = new ] && sh tools/web_checks/task38d/newsite.sh
site=$here/site_$side
port=9640 http=8194 gw=9641

# the server: a fresh copy, practice minutes and password per case
practice=20 pass=
[ "$case" = session ] && practice=1
[ "$case" = refused ] && pass=test-pass-38d
srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http -v practice=$practice -v pass="$pass" '
    /^\[/ { section = $0 }
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-38d"; next }
    /^PASSWORD=/ { print "PASSWORD=" pass; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    section == "[PRACTICE]" && /^TIME=/ { sub(/=.*/, "=" practice) }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }

(cd "$srv" && exec timeout 400 ./acServer.exe > "../mp_${side}_$case/server.log" 2>&1) &
server_job=$!
target/release/rustyac-gateway.exe --name gw --listen 127.0.0.1:$gw --server 127.0.0.1:$port > "$run/gateway.log" 2>&1 &
gw_job=$!
# a site per client (B's may carry a fault)
py web/serve.py --site "$site" --pack $here/pack --port 8188 --quiet > "$run/serve_a.log" 2>&1 &
sa=$!
fault=
case $case in cutjoin | cutlate) fault="--fault cut:ks_laguna_seca.kn5" ;; esac
# shellcheck disable=SC2086
py web/serve.py --site "$site" --pack $here/pack --port 8189 $fault > "$run/serve_b.log" 2>&1 &
sb=$!
sleep 4

cars="&servercars=ks_ferrari_f2004,bmw_z4_gt3&servertrack=ks_laguna_seca"
[ "$case" = cutlate ] && cars=
addr() { # addr <site port> <name> [more]
    echo "http://127.0.0.1:$1/?join=address&server=ws://127.0.0.1:$gw/net/laguna$cars&autodrive=1&spawn=pit&mpcar=ks_ferrari_f2004&name=$2$3"
}
client() { # client <name> <site port> <seconds> [check.mjs args]: in the background
    name=$1 sport=$2 secs=$3
    shift 3
    extra=
    case $name in a) cdp=9351 ;; b) cdp=9352 ;; *) cdp=9353 ;; esac
    [ -n "$pass" ] && extra="&password=$pass"
    [ "$name" = c ] && extra="&password=wrong"
    node web/check.mjs --url "$(addr "$sport" "$name" "$extra")" --seconds "$secs" --profile "$here/prof_mp_$name" --port "$cdp" --timeout 300 --script tools/web_checks/task38d/mpdiag.js "$@" > "$run/$name.json" 2> "$run/$name.err" &
}

case $case in
cutjoin | cutlate)
    client a 8188 90; a=$!
    sleep 20
    client b 8189 60; b=$!
    wait $a $b ;;
gwclose)
    client a 8188 80; a=$!
    sleep 50
    kill $gw_job; echo "gateway stopped at $(date +%T)" >> "$run/gateway.log"
    wait $a ;;
refused)
    client a 8188 80; a=$!
    client b 8189 80; b=$!
    sleep 40
    client c 8188 5; c=$!
    wait $a $b $c ;;
freeze)
    client a 8188 150; a=$!
    client b 8189 120 --freeze 20@60; b=$!
    wait $a $b ;;
session)
    client a 8188 120; a=$!
    client b 8189 120; b=$!
    wait $a $b ;;
esac
kill $gw_job $sa $sb $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
wait 2>/dev/null
rm -rf "$here/prof_mp_"* "$srv"

# what each side says
{
    echo "== $side $case"
    grep -E "Clean exit|disconnected|NEW PICKUP|session|SESSION|KICK|kick|Removing|LAP " "$run/server.log" | grep -v "^$" | head -30
    grep -E " left after |joined|stopped" "$run/gateway.log" | head -20
    for f in "$run"/?.json; do
        [ -f "$f" ] || continue
        py - "$f" <<'PY'
import json, sys, os
f = sys.argv[1]
try:
    r = json.load(open(f, encoding='utf-8'))
except Exception as e:
    print(os.path.basename(f), 'no report', e); sys.exit()
s = r.get('script') or {}
print(f"{os.path.basename(f)[0]}: state {r.get('state')} | error {r.get('error')} | frozen {r.get('frozen')} | mp: {s.get('mp')} | online: {s.get('online')}")
for e in s.get('diag') or []:
    if any(k in e for k in ('net:', 'error:', 'download:')):
        print('   ', e[:300])
PY
    done
} > "$run/summary.txt"
cat "$run/summary.txt"
