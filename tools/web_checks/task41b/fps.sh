#!/bin/bash
# Task 41b A.3: frame rate with sound, Task 41's page (re/scratch/task41/site) against this one
# (re/scratch/task41b/site), and sound off: Task 41's fps.sh drive (f2004 at Spa, autodrive,
# 40 s, headless Chrome) on Task 41's pack, twice each, one at a time.
cd "$(dirname "$0")/../../.."
D=re/scratch/task41b
mkdir -p $D/fps
one() { # one <name> <site> <query>
    py -3 web/serve.py --port 8163 --site $2 --pack re/scratch/task41/pack --quiet > $D/fps/serve.log 2>&1 &
    sp=$!
    sleep 4
    node web/check.mjs --url "http://127.0.0.1:8163/?go=1&autodrive=1&car=ks_ferrari_f2004&track=spa&$3" --seconds 40 --script re/scratch/task41/page/fps_script.js --profile $D/fps_profile --port 9363 --timeout 900 --flags "--autoplay-policy=no-user-gesture-required" > $D/fps/$1.json 2> $D/fps/$1.err
    kill $sp 2>/dev/null
    wait $sp 2>/dev/null
    node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); console.log(process.argv[2], "|", r.backend, "| fps", r.frames_per_second, "| ms/frame", r.work_ms_per_frame, "| steps/s", r.physics_steps_per_second, "| sound", r.script && r.script.sound)' $D/fps/$1.json $1
}
for n in 1 2; do
    one old_on_$n re/scratch/task41/site ""
    one new_on_$n $D/site ""
    one new_off_$n $D/site "sound=0"
done
rm -rf $D/fps_profile
