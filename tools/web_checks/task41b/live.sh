#!/bin/sh
# Task 41b A.1: a 2-client online session's sound. Two headless Chromes join one server, all on
# 127.0.0.1: Kunos' acServer (a copy of dist-server, Spa, no lobby, made-up admin password), the
# gateway, web/serve.py with Task 41's pack (FMOD 2.03 HTML5 in it). Each page keeps a sound tape
# and its FMOD call log (?soundtape=5), then the desktop hears each tape with FMOD 1.08
# (rustyac.exe --sound-tape), plain and with the page's answers scripted in.
#   sh tools/web_checks/task41b/live.sh   (needs the site in re/scratch/task41b/site, rustyac.exe, the gateway)
cd "$(dirname "$0")/../../.." || exit 1
here=re/scratch/task41b
run=$here/live
rm -rf "$run" && mkdir -p "$run/empty_cfg" "$run/empty_docs"
port=9650 http=8196 gw=9651 web=8197

srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/spa/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http '
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-41b"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    { print }' dist-server/cfg/spa/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }

(cd "$srv" && exec timeout 400 ./acServer.exe > "../live/server.log" 2>&1) &
server_job=$!
target/release/rustyac-gateway.exe --name gw --listen 127.0.0.1:$gw --server 127.0.0.1:$port > "$run/gateway.log" 2>&1 &
gw_job=$!
py web/serve.py --port $web --site $here/site --pack re/scratch/task41/pack --quiet > "$run/serve.log" 2>&1 &
sv=$!
sleep 5

echo 'return window.rustyac.soundTape();' > "$run/end.js"
client() { # client <name> <car> <cdp port> <seconds>: in the background
    node web/check.mjs --url "http://127.0.0.1:$web/?join=address&server=ws://127.0.0.1:$gw/net/spa&servercars=ks_ferrari_f2004,bmw_z4_gt3&servertrack=spa&autodrive=1&spawn=pit&mpcar=$2&name=$1&soundtape=5" \
        --seconds "$4" --profile "$here/prof_live_$1" --port "$3" --timeout 900 --flags "--autoplay-policy=no-user-gesture-required" --script "$run/end.js" > "$run/$1.json" 2> "$run/$1.err" &
}
# (short: a page hands its whole call log back in one DevTools answer, which has a size limit)
client a ks_ferrari_f2004 9371 30; a=$!
sleep 8
client b bmw_z4_gt3 9372 20; b=$!
wait $a $b
kill $gw_job $sv $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
wait 2>/dev/null
rm -rf "$here/prof_live_"* "$srv"

for c in a b; do
    node -e '
const fs = require("fs");
const r = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const s = r.script || {};
if (!s.tape) { console.log("no tape:", r.error, JSON.stringify(r.script).slice(0, 300)); process.exit(); }
fs.writeFileSync(process.argv[2], Buffer.from(s.tape, "base64"));
fs.writeFileSync(process.argv[3], s.log);
console.log(`tape ${Buffer.from(s.tape, "base64").length} bytes, log ${s.log.split("\n").length} lines, state ${r.state}, error ${r.error}`);
' "$run/$c.json" "$run/$c.soundtape" "$run/page_$c.log" > "$run/$c.txt" 2>&1
    rm -f "$run/$c.json"
    desk() { target/release/rustyac.exe --sound-tape "$run/$c.soundtape" --audio-wav "$run/w.wav" --audio-log "$1" --cfg-dir "$run/empty_cfg" --documents "$run/empty_docs" --no-steam "${@:2}" 2>&1 | grep -E "audio log|^audio: [0-9]+ other|^error|no sound"; }
    banks=$(grep -o 'studio_load_bank_file h1 "[^"]*"' "$run/page_$c.log" | sed 's/.*"\(.*\)"/\1/' | tr '\n' ' ')
    desk "$run/desk_$c.log" > "$run/desk_$c.txt"
    # the setups differ (the page plays on the browser's sound card, the desktop writes a WAV):
    # the frames are compared, and the desktop's script is its own setup and the page's frames
    frames() { sed -n '/^# frame 0$/,$p' "$1"; }
    { sed -n '/^# frame 0$/q;p' "$run/desk_$c.log"; frames "$run/page_$c.log"; } > "$run/script_$c.log"
    desk "$run/desk_${c}_s.log" --audio-script "$run/script_$c.log" >> "$run/desk_$c.txt"
    for f in page_$c desk_$c desk_${c}_s; do frames "$run/$f.log" > "$run/$f.frames"; done
    echo "$c | $(cat "$run/$c.txt") | banks: $banks | desktop: $(py tools/proofs/logdiff.py "$run/desk_$c.frames" "$run/page_$c.frames" | head -1 | cut -c1-90) | desktop with the page's answers: $(py tools/proofs/logdiff.py "$run/desk_${c}_s.frames" "$run/page_$c.frames" | head -1)"
done | tee "$run/summary.txt"
grep -E "NEW PICKUP|disconnected|Clean exit" "$run/server.log" | head -8
rm -f "$run/w.wav" "$run"/*.frames "$run"/script_?.log
