#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Records the five multiplayer tapes of Tasks 25-26 again, from live sessions of Kunos'
# acServer and rustyac.exe clients on 127.0.0.1 (docs/port/multiplayer.md, "Golden tapes"):
#   laguna: two F2004 clients drive 200 s at Laguna Seca            -> laguna_a, laguna_b
#   rich:   four clients, 150 s, a one-minute practice: a and b drive; d (Z4 GT3) joins at
#           10 s, c at 15 s; three of them chat; d is killed at 40 s; c leaves at 55 s;
#           practice ends at 60 s and qualifying starts             -> rich_a, rich_b, rich_c
# Tapes go to re/gold/net/tapes/ (git-ignored, permanent). Needs dist-server/ (made by
# `python server/make_server_pack.py --windows`) and target/release/rustyac.exe.
#
#   sh tools/net_gold/record.sh [laguna|rich|all]
set -eu
repo=$(cd "$(dirname "$0")/../.." && pwd)
work=${NET_GOLD_WORK:-$repo/target/proofs/net_work}
out=$repo/re/gold/net/tapes
what=${1:-all}
mkdir -p "$work" "$out"

# a client is a copy of the release exe (the build may change while a session runs)
rm -rf "$work/bin" && mkdir -p "$work/bin"
cp "$repo/target/release/rustyac.exe" "$work/bin/"

# server <name> <tcp/udp port> <http port> <practice minutes>: a fresh copy of the kit's server
server() {
    d=$work/server_$1
    rm -rf "$d" && mkdir -p "$d/cfg"
    cp -r "$repo/dist-server/acServer.exe" "$repo/dist-server/content" "$repo/dist-server/system" "$d/"
    cp "$repo/dist-server/cfg/laguna_seca/entry_list.ini" "$d/cfg/"
    # a made-up admin password; only the practice's length changes per session
    awk -v port="$2" -v http="$3" -v practice="$4" '
        /^\[/ { section = $0 }
        /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-37"; next }
        /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
        /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
        section == "[PRACTICE]" && /^TIME=/ { sub(/=.*/, "=" practice) }
        { print }' "$repo/dist-server/cfg/laguna_seca/server_cfg.ini" > "$d/cfg/server_cfg.ini"
    grep -q "REGISTER_TO_LOBBY=0" "$d/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }
}

# client <session> <name> <seconds> <tape or -> <car> [more rustyac.exe args]: one headless
# driver in the background, its log in the session's folder
client() {
    s=$1 name=$2 secs=$3 tape=$4 car=$5
    shift 5
    tape_args=
    [ "$tape" = - ] || tape_args="--net-tape $out/$tape.nettape"
    # shellcheck disable=SC2086
    "$work/bin/rustyac.exe" --headless --autodrive --spawn start --duration "$secs" --no-audio --no-shm \
        --no-race-ini --join "127.0.0.1:$port" --car "$car" --no-steam --name "$name" \
        --net-identity "$work/session_$s/$name.ini" $tape_args "$@" > "$work/session_$s/$name.log" 2>&1 &
}

# start_server <session> <seconds>: the server in the background; `timeout` ends it
start_server() {
    mkdir -p "$work/session_$1"
    (cd "$work/server_$1" && exec timeout "$2" ./acServer.exe > "$work/session_$1/server.log" 2>&1) &
    server_job=$!
    sleep 4
}

if [ "$what" = laguna ] || [ "$what" = all ]; then
    port=9630
    server laguna $port 8093 20
    start_server laguna 230
    client laguna tester1 200 laguna_a ks_ferrari_f2004
    a=$!
    sleep 1
    client laguna tester2 200 laguna_b ks_ferrari_f2004
    b=$!
    wait $a $b || true
    wait $server_job 2>/dev/null || true
    echo "laguna: $(ls -l "$out"/laguna_*.nettape | wc -l) tapes"
fi

if [ "$what" = rich ] || [ "$what" = all ]; then
    port=9631
    server rich $port 8094 1
    start_server rich 175
    client rich tester1 150 rich_a ks_ferrari_f2004 --chat "first@20"
    a=$!
    sleep 1
    client rich tester2 150 rich_b ks_ferrari_f2004 --chat "second@30"
    b=$!
    sleep 9
    # d: a Z4 GT3 that joins at 10 s and is killed at 40 s (no clean leave)
    client rich tester4 150 - bmw_z4_gt3
    d=$!
    sleep 5
    # c: joins at 15 s, chats, leaves cleanly at 55 s
    client rich tester3 40 rich_c ks_ferrari_f2004 --chat "third@10"
    c=$!
    sleep 25
    taskkill //F //PID "$(cat /proc/$d/winpid)" > /dev/null
    wait $c $a $b || true
    wait $server_job 2>/dev/null || true
    grep -h "COLLISION BETWEEN" "$work/session_rich/server.log" || echo "rich: no collision this time"
    echo "rich: $(ls -l "$out"/rich_*.nettape | wc -l) tapes"
fi
