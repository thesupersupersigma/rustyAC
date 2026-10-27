#!/bin/bash
# Task 40: the page driving (autodrive) in Full mode with AC's HUD and 3 apps open, on the GPU:
# frame rate per graphics preset, and a screenshot.
# usage: sh tools/web_checks/task40/fps.sh <name> <graphics spec> [seconds] [size] [extra query]
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
S=re/scratch/task40; O=$S/fps
mkdir -p $O
name=$1; gfx=$2; secs=${3:-40}; size=${4:-1920x1080}; extra=$5
apps=$(node -e 'process.stdout.write(encodeURIComponent("show:essentials;show:tyres;show:pedals"))')
q="backend=full&car=ks_ferrari_f2004&track=magione&autodrive=1&graphics=$gfx&guido=$apps$extra"
py -3 web/serve.py --port 8141 --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $O/$name.serve.log 2>&1 &
sp=$!
sleep 4
node web/check.mjs --url "http://127.0.0.1:8141/?testfolder=/ac-test/&go=1&$q" --seconds $secs --size $size --screenshot $O/$name.png --profile $S/chrome_profile --port 9341 --flags "--enable-features=Vulkan" > $O/$name.json 2> $O/$name.err
kill $sp 2>/dev/null
python -c "
import json
d=json.load(open('$O/$name.json',encoding='utf-8'))
print('$name', '$gfx', '$size', 'fps', d.get('frames_per_second'), 'work ms', d.get('work_ms_per_frame'), 'kmh', d.get('kmh'), 'error', d.get('error'))
"
