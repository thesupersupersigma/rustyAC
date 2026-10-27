#!/bin/bash
# Task 42c step 4: the page alone on a local acServer copy (127.0.0.1 only) through the gateway,
# served with a stand-in install that allows the free camera (re/scratch/task42c/s4/acroot).
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
here=re/scratch/task42c/s4
run=$here/run
rm -rf "$run" && mkdir -p "$run"
port=9670 http=8216 gw=9671 site=8191
srv=$here/srv
rm -rf "$srv" && mkdir -p "$srv/cfg"
cp -r dist-server/acServer.exe dist-server/content dist-server/system "$srv/"
cp dist-server/cfg/laguna_seca/entry_list.ini "$srv/cfg/"
awk -v port=$port -v http=$http '
    /^ADMIN_PASSWORD=/ { print "ADMIN_PASSWORD=test-admin-42c4"; next }
    /^PASSWORD=/ { print "PASSWORD="; next }
    /^(TCP|UDP)_PORT=/ { sub(/=.*/, "=" port) }
    /^HTTP_PORT=/ { sub(/=.*/, "=" http) }
    { print }' dist-server/cfg/laguna_seca/server_cfg.ini > "$srv/cfg/server_cfg.ini"
grep -q "REGISTER_TO_LOBBY=0" "$srv/cfg/server_cfg.ini" || { echo "the server would register to the lobby" >&2; exit 1; }
(cd "$srv" && exec timeout 600 ./acServer.exe > "../run/server.log" 2>&1) &
server_job=$!
target/release/rustyac-gateway.exe --name gw --listen 127.0.0.1:$gw --server 127.0.0.1:$port > "$run/gateway.log" 2>&1 &
gw_job=$!
py web/serve.py --ac "$(pwd -W)/$here/acroot" --port $site --quiet > "$run/serve.log" 2>&1 &
sv=$!
sleep 4
python tools/web_checks/task42c/s4/mkflow.py "$run/flow.json" "$run"
node web/check.mjs --url "http://127.0.0.1:$site/?testfolder=/ac-test/&launcher=0&join=address&server=ws://127.0.0.1:$gw/net/laguna&servercars=ks_ferrari_f2004,bmw_z4_gt3&servertrack=ks_laguna_seca&spawn=pit&mpcar=ks_ferrari_f2004&name=b&backend=full" \
    --flow "$run/flow.json" --profile "$here/prof" --port 9373 --timeout 400 --flags "${FLAGS:---enable-features=Vulkan}" > "$run/b.json" 2> "$run/b.err"
echo "check.mjs exit $?"
kill $gw_job $sv $server_job 2>/dev/null
taskkill //F //IM acServer.exe > /dev/null 2>&1
wait 2>/dev/null
rm -rf "$here/prof" "$srv"
py -c "
import json
r = json.load(open('$run/b.json'))
for s in r['flow']:
    if 'value' in s or 'error' in s or 'failed' in str(s): print({k: str(v)[:200] for k, v in s.items() if k in ('js','value','error','cdp','at')})
print('console', [l for l in r.get('console', []) if 'rror' in str(l)][:10])"
