#!/bin/bash
# Task 43 step 4: the chat lines and votes rustyAC sends, against the game's own sendChat /
# sendVote. A live session on 127.0.0.1 (Kunos' acServer, a copy of dist-server, Laguna Seca):
# rustyac.exe "a" (taped) and "b" drive; a chats and votes. Then car_oracle net hands every chat
# line and vote of a's tape to the game's ACClient::sendChat / sendVote and compares the bytes.
#   sh tools/web_checks/task43/s4/votes.sh
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
here=re/scratch/task43/s4
run=$here/run
rm -rf "$run" && mkdir -p "$run"
port=9662 http=8198

srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http '
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-43"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }

(cd "$srv" && exec timeout 120 ./acServer.exe > "../run/server.log" 2>&1) &
server_job=$!
sleep 4
client() { # client <name> <tape or -> [args]
    name=$1 tape=$2
    shift 2
    t=
    [ "$tape" = - ] || t="--net-tape $tape"
    # shellcheck disable=SC2086
    MSYS_NO_PATHCONV=1 target/release/rustyac.exe --headless --autodrive --spawn start --duration 70 --no-audio --no-shm \
        --no-race-ini --join 127.0.0.1:$port --car ks_ferrari_f2004 --no-steam --name "$name" $t "$@" > "$run/$name.log" 2>&1
}
client b - &
b=$!
sleep 2
# short and long lines, one not in ASCII, then every kind of vote (the kick of a driver who is
# not there goes out with target 0xff)
client a "$run/a.nettape" --chat "hi@10" --chat "hello from rustyAC, a longer line@13" --chat "ciao, è già@16" \
    --chat "/vote next yes@20" --chat "/vote restart no@23" --chat "/vote kick nobody yes@26" --chat "/vote kick b no@29" \
    --chat "/vote restart yes@32" --chat "/vote next no@35"
wait $b
wait $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
rm -rf "$srv"
grep -E "^online: (vote|not a vote)" "$run/a.log" > "$run/a_votes.txt"
grep -iE "vote|chat" "$run/server.log" | head -30 > "$run/server_votes.txt"
tools/car_oracle/target/release/car_oracle.exe net "$run/a.nettape" --scenario net_watch --track ks_laguna_seca --steps 200 --out "$run/oracle" > "$run/oracle.log" 2>&1
{
    echo "== a's votes (log)"; cat "$run/a_votes.txt"
    echo "== server"; cat "$run/server_votes.txt"
    echo "== oracle"; grep -E "chat lines|id 0x(47|64|65|66)|IDENTICAL|DIFFERENT|differ|game:|port:" "$run/oracle.log"
} | tee "$run/summary.txt"
