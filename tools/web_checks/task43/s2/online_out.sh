#!/bin/bash
# Task 43 step 2: race_out.json after an online session. A live session on 127.0.0.1 (Kunos'
# acServer, a copy of dist-server, Laguna Seca: practice 2 min, qualifying 2 min, a 1-lap race):
# rustyac.exe "a" and "b" drive through all three; each writes race_out.json into its own folder
# when it leaves. acServer's own results files (results/*.json) are kept beside them.
#   sh tools/web_checks/task43/s2/online_out.sh [seconds]   (SERVER_EXTRA=s: the server stops s after
#   the clients' time, 90 by default; a small value makes it go away under them)
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
here=re/scratch/task43/s2
run=$here/run
rm -rf "$run" && mkdir -p "$run"
port=9664 http=8199 secs=${1:-420}

srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http '
    /^\[/ { section = $0 }
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-43"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    /^LOOP_MODE=/ { print "LOOP_MODE=0"; next }
    /^RACE_OVER_TIME=/ { print "RACE_OVER_TIME=30"; next }
    /^RESULT_SCREEN_TIME=/ { print "RESULT_SCREEN_TIME=5"; next }
    /^TIME=/ && (section == "[PRACTICE]" || section == "[QUALIFY]") { print "TIME=2"; next }
    /^LAPS=/ { print "LAPS=1"; next }
    /^WAIT_TIME=/ { print "WAIT_TIME=20"; next }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }

(cd "$srv" && exec timeout $((secs + ${SERVER_EXTRA:-90})) ./acServer.exe > "../run/server.log" 2>&1) &
server_job=$!
sleep 4
client() { # client <name> [args]
    name=$1
    shift
    MSYS_NO_PATHCONV=1 target/release/rustyac.exe --headless --autodrive --duration "$secs" --no-audio --no-shm \
        --no-race-ini --join 127.0.0.1:$port --car ks_ferrari_f2004 --no-steam --name "$name" --out-dir "$run/out_$name" "$@" > "$run/$name.log" 2>&1
}
client b &
b=$!
sleep 2
client a
wait $b
wait $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
cp -r "$srv/results" "$run/server_results" 2>/dev/null
rm -rf "$srv"
{
    for name in a b; do
        echo "== $name"
        grep -E "^online: (a new session|the results|race_out|wrote|could not)" "$run/$name.log"
    done
    echo "== acServer results files"; ls "$run/server_results" 2>/dev/null
} | tee "$run/summary.txt"
