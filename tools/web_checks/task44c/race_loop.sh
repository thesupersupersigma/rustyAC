#!/bin/bash
# Task 44c step 9: race_out.json after an online server starts its next race. Kunos' acServer on
# 127.0.0.1 (a copy of dist-server, Laguna Seca), races only, LOOP_MODE=1, 1 lap, a long
# RACE_OVER_TIME so the clients can leave between the second race's end and the third's start.
# rustyac.exe "a" and "b" (F2004, autodrive) drive; acServer's results files are kept beside theirs.
#   sh tools/web_checks/task44c/race_loop.sh [seconds]
#   py -I tools/web_checks/task43/s2/compare.py re/scratch/task44c/s9/run   (the last race's file)
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
here=re/scratch/task44c/s9
run=$here/run
rm -rf "$run" && mkdir -p "$run"
port=9665 http=8200 secs=${1:-430}

srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http '
    /^\[/ { section = $0 }
    section == "[PRACTICE]" || section == "[QUALIFY]" { next }
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-44c"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    /^LOOP_MODE=/ { print "LOOP_MODE=1"; next }
    /^RACE_OVER_TIME=/ { print "RACE_OVER_TIME=150"; next }
    /^RESULT_SCREEN_TIME=/ { print "RESULT_SCREEN_TIME=5"; next }
    /^LAPS=/ { print "LAPS=1"; next }
    /^WAIT_TIME=/ { print "WAIT_TIME=20"; next }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }

(cd "$srv" && exec timeout $((secs + 60)) ./acServer.exe > "../run/server.log" 2>&1) &
server_job=$!
sleep 4
client() { # client <name>
    MSYS_NO_PATHCONV=1 target/release/rustyac.exe --headless --autodrive --duration "$secs" --no-audio --no-shm \
        --no-race-ini --join 127.0.0.1:$port --car ks_ferrari_f2004 --no-steam --name "$1" --out-dir "$run/out_$1" > "$run/$1.log" 2>&1
}
client b &
b=$!
sleep 2
client a
wait $b
taskkill //F //IM acServer.exe > /dev/null 2>&1
wait $server_job 2>/dev/null
cp -r "$srv/results" "$run/server_results_all" 2>/dev/null
rm -rf "$srv"
# compare.py reads one file a type: the last race's
mkdir -p "$run/server_results"
last=$(ls "$run/server_results_all"/*_RACE.json 2>/dev/null | tail -n 1)
[ -n "$last" ] && cp "$last" "$run/server_results/"
{
    for name in a b; do
        echo "== $name"
        grep -E "^online: (a new session|the results|race_out|wrote|could not)" "$run/$name.log"
    done
    echo "== acServer results files"; ls "$run/server_results_all" 2>/dev/null
} | tee "$run/summary.txt"
