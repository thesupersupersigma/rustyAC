#!/bin/bash
# Task 39: the graphics settings' proofs. Full mode's fixed frame (f2004 at Magione from the pit,
# 3 frames) in the page (?graphics=<spec>, headless Chrome on SwiftShader) against the native
# full_frame example with the same video.ini on kgl's wgpu (SwiftShader) and Direct3D 11 WARP;
# then one setting at a time against High (Direct3D 11 WARP): its call log must change.
# usage: gfx.sh [build|page|native|settings]...   (default: all)
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
G=re/scratch/task39/gfx; F=$G/frames
export CARGO_BUILD_JOBS=4
mkdir -p $F
CAR=ks_ferrari_f2004; TRACK=magione
MIX='high,VIDEO/SHADOW_MAP_SIZE=4096,ASSETTOCORSA/WORLD_DETAIL=2,VIDEO/ANISOTROPIC=16,EFFECTS/SMOKE=5,MIRROR/SIZE=1024,MIRROR/HQ=1,CUBEMAP/SIZE=1024,CUBEMAP/FACES_PER_FRAME=3,ASSETTOCORSA/HIDE_ARMS=1'
SWIFT="$(cygpath -w "$PWD/re/scratch/task38b/swiftshader/vk_swiftshader_icd.json")"
size() { cat $G/size.txt 2>/dev/null || echo 1280x720; }
d3d() { # d3d <name> [full_frame args]
    local name=$1; shift
    rm -rf target/release/examples/rustyac_cache
    target/release/examples/full_frame.exe --car $CAR --track $TRACK --spawn pit --size $(size) --frames 3 --warp --out $F --name $name "$@" 2>&1 | tail -n 1 | cut -c1-160
}
wgpu() {
    local name=$1; shift
    rm -rf target/full_wgpu/release/examples/rustyac_cache
    VK_DRIVER_FILES="$SWIFT" RUSTYAC_WGPU_ADAPTER=swiftshader target/full_wgpu/release/examples/full_frame.exe --car $CAR --track $TRACK --spawn pit --size $(size) --frames 3 --out $F --name $name "$@" 2>&1 | tail -n 1 | cut -c1-160
}
same() { cmp -s $F/$1.kgllog $F/$2.kgllog && echo "IDENTICAL $1 == $2 ($(wc -l < $F/$1.kgllog) lines)" || echo "DIFFERENT $1 != $2 ($(wc -l < $F/$1.kgllog) / $(wc -l < $F/$2.kgllog) lines)"; }
steps=${*:-build page native settings}
for s in $steps; do case $s in
build)
    cargo build --release --locked --quiet -p rustyac-web --example full_frame || exit 2
    CARGO_TARGET_DIR=target/full_wgpu cargo build --release --locked --quiet -p rustyac-web --example full_frame --features wgpu || exit 2
    ;;
page)
    # (the page was built by the run before: python web/build.py)
    for spec in high low "$MIX" "low,SCALE=50"; do
        name=page_$(echo "${spec%%,*}" | tr -d ' ')$( [ "$spec" = "$MIX" ] && echo _mix )$( [ "$spec" = "low,SCALE=50" ] && echo _50 )
        py -3 web/serve.py --port 8161 --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $G/$name.serve.log 2>&1 &
        sp=$!
        sleep 5
        q="testfolder=/ac-test/&go=1&car=$CAR&track=$TRACK&spawn=pit&backend=full&fullproof=3&graphics=$(node -e 'process.stdout.write(encodeURIComponent(process.argv[1]))' "$spec")"
        node web/check.mjs --url "http://127.0.0.1:8161/?$q" --script tools/web_checks/task40/full_proof.js --screenshot $G/$name.png --profile $G/chrome_profile --port 9361 --timeout 1200 --flags "--use-webgpu-adapter=swiftshader" > $G/$name.json 2> $G/$name.err
        echo "$name: check.mjs exit $?"
        kill $sp 2>/dev/null
        rm -rf $G/chrome_profile
        python - $G $name <<'EOF'
import json, sys
g, name = sys.argv[1:]
r = json.load(open(f'{g}/{name}.json', encoding='utf-8'))
s = json.loads(r['script'])
log = s['log'] if s['log'].endswith('\n') else s['log'] + '\n'
open(f'{g}/frames/{name}.kgllog', 'w', newline='\n').write(log)
size = s['size'].replace(' ', 'x')
if not name.endswith('_50'):
    open(f'{g}/size.txt', 'w').write(size)
print(f'{name}: size {size}, errors {r.get("console")}')
EOF
    done
    ;;
native)
    d3d none_d3d
    for spec in high low "$MIX"; do
        tag=$(echo "${spec%%,*}")$( [ "$spec" = "$MIX" ] && echo _mix )
        node tools/web_checks/task39/gfx_ini.mjs "$spec" $G/$tag.ini
        d3d ${tag}_d3d --video-ini $G/$tag.ini
        wgpu ${tag}_wgpu --video-ini $G/$tag.ini
        same page_$tag ${tag}_wgpu
        same ${tag}_wgpu ${tag}_d3d
    done
    same none_d3d high_d3d
    ;;
settings)
    # one setting off High (or off its base: what it needs to show at all)
    while read -r key value base; do
        [ -z "$key" ] && continue
        b=${base:-high}
        bt=$(echo "$b" | tr ',/=' '_._')
        [ -f $F/set_$bt.kgllog ] || { node tools/web_checks/task39/gfx_ini.mjs "$b" $G/set_$bt.ini; d3d set_$bt --video-ini $G/set_$bt.ini > /dev/null; }
        t=$(echo "$key=$value" | tr '/=' '._')
        node tools/web_checks/task39/gfx_ini.mjs "$b,$key=$value" $G/set_$t.ini
        d3d set_$t --video-ini $G/set_$t.ini > /dev/null
        echo "$key=$value (from $b): $(same set_$t set_$bt)"
    done <<'EOF'
VIDEO/ANISOTROPIC 16
VIDEO/AASAMPLES 4
ASSETTOCORSA/WORLD_DETAIL 0
VIDEO/SHADOW_MAP_SIZE 4096
EFFECTS/SMOKE 0
EFFECTS/RENDER_SMOKE_IN_MIRROR 1 high,MIRROR/SIZE=512
POST_PROCESS/ENABLED 1
POST_PROCESS/FXAA 1 high,POST_PROCESS/ENABLED=1
EFFECTS/MOTION_BLUR 6 high,POST_PROCESS/ENABLED=1
ASSETTOCORSA/HIDE_ARMS 1
ASSETTOCORSA/LOCK_STEER 1
ASSETTOCORSA/HIDE_STEER 1
MIRROR/SIZE 512
MIRROR/HQ 1 high,MIRROR/SIZE=512
CUBEMAP/SIZE 1024
CUBEMAP/FACES_PER_FRAME 3
EOF
    ;;
extra)
    # Low and Low at 50 %: the page draws them at its render scale's size
    cargo build --release --locked --quiet -p rustyac-web --example full_frame || exit 2
    CARGO_TARGET_DIR=target/full_wgpu cargo build --release --locked --quiet -p rustyac-web --example full_frame --features wgpu || exit 2
    for pair in "low 960x540" "low_50 640x360"; do
        set -- $pair
        echo "$2" > $G/size.txt
        node tools/web_checks/task39/gfx_ini.mjs low $G/low.ini
        d3d ${1}_s_d3d --video-ini $G/low.ini
        wgpu ${1}_s_wgpu --video-ini $G/low.ini
        same page_$1 ${1}_s_wgpu
        same ${1}_s_wgpu ${1}_s_d3d
    done
    echo 1280x720 > $G/size.txt
    # MSAA reaches the call log only with post-processing (else it is the swap chain's); the
    # cockpit's three need the cockpit camera
    b=high,POST_PROCESS/ENABLED=1; bt=high_POST_PROCESS.ENABLED_1
    node tools/web_checks/task39/gfx_ini.mjs "$b,VIDEO/AASAMPLES=4" $G/set_aa4p.ini
    d3d set_aa4p --video-ini $G/set_aa4p.ini > /dev/null
    echo "VIDEO/AASAMPLES=4 (from $b): $(same set_aa4p set_$bt)"
    node tools/web_checks/task39/gfx_ini.mjs high $G/set_c_high.ini
    d3d set_c_high --video-ini $G/set_c_high.ini --cockpit > /dev/null
    for key in ASSETTOCORSA/HIDE_ARMS ASSETTOCORSA/LOCK_STEER ASSETTOCORSA/HIDE_STEER; do
        t=c_$(echo "$key" | tr '/' '.')
        node tools/web_checks/task39/gfx_ini.mjs "high,$key=1" $G/set_$t.ini
        d3d set_$t --video-ini $G/set_$t.ini --cockpit > /dev/null
        echo "$key=1 (from high, cockpit camera): $(same set_$t set_c_high)"
    done
    ;;
esac; done
