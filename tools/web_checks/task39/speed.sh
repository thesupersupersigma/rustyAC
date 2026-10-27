#!/bin/bash
# Task 39: the speed table. A 20 s Full drive (autodrive, f2004 at Magione from the test pack)
# at 1920 x 1080 for each preset, on this PC's GPU and on SwiftShader.
# usage: speed.sh [gpu|swift]...   (default: both)
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
G=re/scratch/task39/speed; mkdir -p $G
PACK=re/scratch/task38i/pack
for where in ${*:-gpu swift}; do
    flags=$([ $where = gpu ] && echo "--enable-features=Vulkan" || echo "--use-webgpu-adapter=swiftshader")
    for spec in high medium low "low,SCALE=50"; do
        name=${where}_$(echo "$spec" | tr ',=' '__')
        py -3 web/serve.py --port 8171 --pack $PACK --quiet > $G/$name.serve.log 2>&1 &
        sp=$!
        sleep 4
        node web/check.mjs --url "http://127.0.0.1:8171/?go=1&car=ks_ferrari_f2004&track=magione&spawn=pit&autodrive=1&backend=full&graphics=$spec" --size 1920x1080 --seconds 20 --profile $G/chrome_profile --port 9371 --timeout 1200 --flags "$flags" > $G/$name.json 2> $G/$name.err
        kill $sp 2>/dev/null
        rm -rf $G/chrome_profile
        node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); const n = (x) => (x == null ? "-" : x.toFixed(1)); console.log(`${process.argv[2]}: ${n(r.frames_per_second)} fps, ${n(r.work_ms_per_frame)} ms/frame, ${r.backend}, state ${r.state}, error ${r.error || "none"}, console ${JSON.stringify(r.console || null).slice(0, 120)}`);' $G/$name.json $name
    done
done
