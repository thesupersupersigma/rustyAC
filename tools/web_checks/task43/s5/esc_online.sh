#!/bin/bash
# Task 43 step 5: "ESC in the last 5 s before the start, online, sends you to the pits and locks
# the controls for 20 s" (ESCMenu::render 0x1400fe420), live. Kunos' acServer on 127.0.0.1
# (Laguna Seca, a race of 1 lap alone, WAIT_TIME 30): rustyac.exe "a" (a window) has the ESC
# menu up from its first frame (--gui-do pits) and on the grid through the countdown, its
# "Drive" pressed at 95 s (after the start: the session starts about 85 s after the server); "b" (headless) just drives. Each writes race_out.json.
#   sh tools/web_checks/task43/s5/esc_online.sh
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
here=re/scratch/task43/s5/esc
run=$PWD/$here/run
rm -rf "$run" && mkdir -p "$run"
port=9666 http=8197 secs=${1:-150}

srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http '
    /^\[/ { section = $0 }
    section == "[PRACTICE]" || section == "[QUALIFY]" { next }
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-43"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    /^LOOP_MODE=/ { print "LOOP_MODE=0"; next }
    /^RACE_OVER_TIME=/ { print "RACE_OVER_TIME=60"; next }
    /^RESULT_SCREEN_TIME=/ { print "RESULT_SCREEN_TIME=5"; next }
    /^LAPS=/ { print "LAPS=1"; next }
    /^WAIT_TIME=/ { print "WAIT_TIME=30"; next }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }
grep -q "^\[PRACTICE\]\|^\[QUALIFY\]" "$srv/cfg/server_cfg.ini" && { echo "not a race alone" >&2; exit 1; }

(cd "$srv" && exec timeout $((secs + 60)) ./acServer.exe > "../run/server.log" 2>&1) &
server_job=$!
sleep 4
client() { # client <name> [args]
    name=$1
    shift
    MSYS_NO_PATHCONV=1 target/release/rustyac.exe --autodrive --duration "$secs" --no-audio --no-shm \
        --no-race-ini --join 127.0.0.1:$port --car ks_ferrari_f2004 --no-steam --name "$name" --out-dir "$run/out_$name" "$@" > "$run/$name.log" 2>&1
}
client b --headless &
b=$!
sleep 2
client a --no-focus --gui-do pits --pit-key drive@95
wait $b
wait $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
rm -rf "$srv"
{
    for name in a b; do
        echo "== $name"
        grep -E "^online: (ESC in|a new session|race_out|wrote)|^(WARNING|error)" "$run/$name.log"
        py -I -c "import json,sys; r=json.load(open(sys.argv[1])); [print('  ', s['name'], [(x['car'], x['time']) for x in s.get('laps', [])], s.get('raceResult')) for s in r['sessions']]" "$run/out_$name/race_out.json" 2>&1
    done
} | tee "$run/summary.txt"
