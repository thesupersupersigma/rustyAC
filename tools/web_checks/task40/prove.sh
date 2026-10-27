#!/bin/bash
# Task 40 step 2: AC's GUI in Full mode's fixed frame, the same --gui-do script three ways:
# native full_frame on Direct3D 11, native full_frame on kgl's wgpu (GPU), the page (headless
# Chrome on the GPU, Vulkan). Logs compared except `fw1 atlas update` lines (the glyph bytes).
# usage: sh tools/web_checks/task40/prove.sh [name...]   (default: every sequence below)
#   env SKIP_NATIVE=1 / SKIP_PAGE=1
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
S=re/scratch/task40; O=$S/prove
mkdir -p $O
CAR=ks_ferrari_f2004; TRACK=magione
# name|frames|sessions|script
SEQ='hud3|3||show:essentials;show:tyres;show:pedals
sidebar|20||show:essentials;show:tyres;show:pedals;mouse:1279:360
esc|3||esc
pits|3||pits
setup|3||pits;click:47:273
pitlane|3|p10|
results|3|p10|results
console|3||console;type:help
tab|3||tab
desktop|3||show:essentials;desktop;show:tyres
names|3||names;damage'
want=" $* "
echo "$SEQ" | while IFS='|' read -r name frames sessions script; do
    [ $# -gt 0 ] && [ "${want#* $name }" = "$want" ] && continue
    extra=""; [ -n "$sessions" ] && extra="--sessions $sessions"
    if [ -z "$SKIP_NATIVE" ]; then
        ./target/release/examples/full_frame.exe --gui "$script" --frames $frames --size 1280x720 --video-ini $S/high.ini --car $CAR --track $TRACK $extra --out $O --name ${name}_d3d > $O/${name}_d3d.out 2>&1 || echo "$name: d3d failed"
        RUSTYAC_WGPU_ADAPTER=gpu ./target/full_wgpu/release/examples/full_frame.exe --gui "$script" --frames $frames --size 1280x720 --video-ini $S/high.ini --car $CAR --track $TRACK $extra --out $O --name ${name}_wgpu > $O/${name}_wgpu.out 2>&1 || echo "$name: wgpu failed"
    fi
    if [ -z "$SKIP_PAGE" ]; then
        q="backend=full&fullproof=$frames&fullgui=$(node -e 'process.stdout.write(encodeURIComponent(process.argv[1]))' "$script")&car=$CAR&track=$TRACK&spawn=pit"
        [ -n "$sessions" ] && q="$q&sessions=$sessions"
        py -3 web/serve.py --port 8141 --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $O/${name}_page.serve.log 2>&1 &
        sp=$!
        sleep 4
        node web/check.mjs --url "http://127.0.0.1:8141/?testfolder=/ac-test/&go=1&$q" --script tools/web_checks/task40/full_proof.js --seconds 2 --size 1280x720 --screenshot $O/${name}_page.png --profile $S/chrome_profile --port 9341 --flags "--enable-features=Vulkan" > $O/${name}_page.json 2> $O/${name}_page.err
        kill $sp 2>/dev/null
        python -c "
import json,sys
d=json.load(open('$O/${name}_page.json',encoding='utf-8'))
try: open('$O/${name}_page.kgllog','w',encoding='utf-8',newline='\n').write(json.loads(d['script'])['log'])
except Exception as e: print('$name: page gave no log:', d.get('error'), e)
"
    fi
    a=$(python tools/web_checks/task40/cmp_log.py $O/${name}_d3d.kgllog $O/${name}_wgpu.kgllog | head -1)
    b=$(python tools/web_checks/task40/cmp_log.py $O/${name}_wgpu.kgllog $O/${name}_page.kgllog | head -1)
    st1=$(diff <(grep "^gui " $O/${name}_d3d.kgllog) <(grep "^gui " $O/${name}_wgpu.kgllog) | grep -c "^>")
    st2=$(diff <(grep "^gui " $O/${name}_wgpu.kgllog) <(grep "^gui " $O/${name}_page.kgllog) | grep -c "^>")
    echo "$name | d3d vs wgpu: $a | wgpu vs page: $b | gui state lines differing: $st1 / $st2"
done
