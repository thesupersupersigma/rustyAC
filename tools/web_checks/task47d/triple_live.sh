#!/bin/bash
# Task 47d step 3, live in Chrome: the Triple screen app's save at the end of a page drive. The
# page's Documents get Task 47c's run6 triple_screen.ini (the desktop run's start file), a Full mode
# drive in triple mode clicks the same spinners as shutdown_files.sh, Esc ends the drive, and the
# file the page saved in its Documents must equal the desktop's (run shutdown_files.sh first).
cd "$(dirname "$0")/../../.." || exit 1
D=re/scratch/task47d/step3
L=$D/live; rm -rf $L; mkdir -p $L
P=${PORT:-8147}
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
S='show:triple_screen;click:220:515;click:220:515;click:220:515;click:40:665;click:560:665;click:560:665'
CAR=${CAR:-bmw_m3_e30} TRACK=${TRACK:-magione}
# the pause menu's Exit at 1920x1080 (EXIT_AT= to stop at the pause menu's picture)
export EXIT_AT=${EXIT_AT-960,692}
py -3 - "$L/flow.json" "$S" "$CAR" "$TRACK" <<'PY'
import base64, json, os, sys, urllib.parse
out, script, car, track = sys.argv[1:]
ini = base64.b64encode(open('re/scratch/task47c/run6/cfg/triple_screen.ini', 'rb').read()).decode()
seed = ("(async () => { const m = await import('/launcher.js'); const r = await import('/pkg/rustyac_web.js');"
        f" const b = Uint8Array.from(atob('{ini}'), (c) => c.charCodeAt(0));"
        " await m.saveDocuments(m.snapshotWith(await m.loadDocuments(), r.Launcher.roots()[1], 'cfg/triple_screen.ini', b, Date.now()));"
        " return b.length; })()")
q = urllib.parse.urlencode({'testfolder': '/ac-test/', 'go': '1', 'backend': 'full', 'car': car, 'track': track,
                            'graphics': 'high,CAMERA/MODE=TRIPLE', 'guido': script})
names = ['cfg/triple_screen.ini', 'cfg/telemetry_user.ini', f'savedData/driverPerformances/{track}/{car}.ini']
read = ("(async () => { const m = await import('/launcher.js'); const s = await m.loadDocuments(); const o = {};"
        f" for (const n of {json.dumps(names)}) {{ const b = m.snapshotFile(s, n); o[n] = b ? btoa(String.fromCharCode(...b)) : null; }}"
        " return JSON.stringify(o); })()")
key = lambda t: {'cdp': 'Input.dispatchKeyEvent', 'params': {'type': t, 'key': 'Escape', 'code': 'Escape', 'windowsVirtualKeyCode': 27}}
# Esc opens AC's pause menu (the game's own), whose EXIT ends the drive as the game closes
steps = [{'until': 'menu'}, {'js': seed}, {'js': f"location.href = '/?{q}'; 1"}, {'until': 'driving'}, {'wait': 8},
         {'shot': out.replace('flow.json', 'drive.png')}, key('rawKeyDown'), key('keyUp'), {'wait': 2},
         {'shot': out.replace('flow.json', 'pause.png')}]
if os.environ.get('EXIT_AT'):
    x, y = map(int, os.environ['EXIT_AT'].split(','))
    mouse = lambda t: {'cdp': 'Input.dispatchMouseEvent', 'params': {'type': t, 'x': x, 'y': y, 'button': 'left', 'clickCount': 1}}
    steps += [mouse('mouseMoved'), {'wait': 0.5}, mouse('mousePressed'), mouse('mouseReleased'), {'until': 'menu'}, {'wait': 3}, {'js': read}]
json.dump(steps, open(out, 'w'))
PY
py -3 web/serve.py --port $P --ac "$AC" --quiet > $L/serve.log 2>&1 &
SP=$!
sleep 4
node web/check.mjs --url "http://127.0.0.1:$P/?testfolder=/ac-test/" --flow $L/flow.json --size 1920x1080 --profile $L/chrome_profile \
    --port $((P+1200)) --flags "--enable-features=Vulkan" > $L/report.json 2> $L/err.txt
echo "chrome exit $?"
kill $SP 2>/dev/null
py -3 - "$L" "$D" <<'PY'
import base64, json, sys
L, D = sys.argv[1:]
flow = json.load(open(f'{L}/report.json'))['flow']
saved = json.loads(flow[-1]['value'])
same = diff = 0
for name, b64 in saved.items():
    desk = f"{D}/desk/own/{name.replace('cfg/', 'cfg/', 1)}"
    try:
        want = open(desk, 'rb').read()
    except OSError:
        want = None
    got = base64.b64decode(b64) if b64 else None
    ok = got is not None and got == want
    same += ok; diff += not ok
    print(f"{'IDENTICAL' if ok else 'DIFFERENT'} {name} (page {len(got) if got else 'none'} bytes, desktop {len(want) if want else 'none'})")
    if got is not None: open(f"{L}/{name.replace('/', '_')}", 'wb').write(got)
print(f'PROOF triple_live identical={same} different={diff}')
PY
