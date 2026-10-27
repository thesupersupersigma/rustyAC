#!/bin/bash
# Task 42c step 3: the Python apps online. A local acServer copy (127.0.0.1 only), desktop "a" runs
# the check app cam42c (stand-in AC folder), desktop "b" chats; the app's log and the console.
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
here=re/scratch/task42c/s3
run=$here/online
W=$(pwd -W)
rm -rf "$run" && mkdir -p "$run/empty_cfg" "$run/docs_a" "$run/docs_b"
port=9660 http=8206
srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http '
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-42c3"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }
(cd "$srv" && exec timeout 300 ./acServer.exe > "../online/server.log" 2>&1) &
server_job=$!
sleep 4
AC_ROOT="$W/$here/acroot" target/release/rustyac.exe --join 127.0.0.1:$port --car ks_ferrari_f2004 --name a --no-steam --cfg-dir "$W/$here/cfg" --documents "$W/$run/docs_a" --duration 30 --windowed --width 1280 --height 720 --bench-shot "$W/$run/a.png" --audio-null > "$run/desk_a.log" 2>&1 &
a=$!
sleep 3
target/release/rustyac.exe --join 127.0.0.1:$port --car ks_ferrari_f2004 --name b --no-steam --cfg-dir "$W/$run/empty_cfg" --documents "$W/$run/docs_b" --duration 24 --windowed --width 640 --height 360 --chat "hello from b@12" --audio-null > "$run/desk_b.log" 2>&1 &
b=$!
wait $a $b
kill $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
wait 2>/dev/null
rm -rf "$srv"
cp $here/acroot/apps/python/cam42c/calls.txt "$run/calls.txt"
grep -iE "chat|python" "$run/desk_a.log" | head -20
cat "$run/calls.txt"
