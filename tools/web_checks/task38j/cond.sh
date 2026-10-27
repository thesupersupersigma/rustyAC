#!/bin/bash
# Task 38j step 2: race.ini conditions (sun 56, time x4, heavy clouds, 9/12 C, grip 86 % +
# 20 groove laps, wind) in Full mode. Physics: desktop record + replay, native race_proof, the
# page's Full drive playing the tape. Picture: the page's Full fixed frame against native wgpu
# (SwiftShader both) and native Direct3D 11 WARP.
# usage: cond.sh [record|physics|frame]...   (default: all three)
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
C=re/scratch/task38j/cond; PACK=re/gold/web/pack; SEED=28005
export CARGO_BUILD_JOBS=4
mkdir -p $C/frames
js() { node -e 'process.stdout.write(JSON.stringify(require("fs").readFileSync(process.argv[1],"utf8")))' "$1"; }
page() { # page <name> <query> <script>
    py -3 web/serve.py --port 8151 --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $C/$1.serve.log 2>&1 &
    local sp=$!
    sleep 5
    node web/check.mjs --url "http://127.0.0.1:8151/?testfolder=/ac-test/&$2" --menu --script "$3" --screenshot $C/$1.png --profile $C/chrome_profile --port 9351 --timeout 1200 --flags "--use-webgpu-adapter=swiftshader" > $C/$1.json 2> $C/$1.err
    echo "$1: check.mjs exit $?"
    kill $sp 2>/dev/null
    rm -rf $C/chrome_profile
}
wait_js='for (let i = 0; i < 600 && !(window.rustyac && window.rustyac.driveRace && !document.getElementById("drive").disabled); i++) await new Promise((r) => setTimeout(r, 100));'
steps=${*:-record physics frame}
for s in $steps; do case $s in
record)
    cargo build --release --locked --quiet -p rustyac-game --bin rustyac || exit 2
    target/release/rustyac.exe --race-ini $C/race.ini --car bmw_z4_gt3 --track ks_laguna_seca --headless --autodrive --record $C/qr.ryin --race-seed $SEED --ai-seed $((SEED-28000)) --duration 20 > $C/record.txt 2>&1
    echo "record: exit $? $(tail -n 1 $C/record.txt)"
    ;;
physics)
    target/release/rustyac.exe --replay $C/qr.ryin --headless --state-hash --physics-threads 8 > $C/desktop.txt 2>&1
    echo "desktop: $(grep '^state hash' $C/desktop.txt)"
    cargo run --release --locked --quiet -p rustyac-web --example race_proof -- --race $C/race.ini --assists $C/assists.ini --seed $SEED --tape $C/qr.ryin --out $C/native > $C/native.txt 2>&1
    echo "native: $(tr '\n' ' ' < $C/native.txt)"
    python web/build.py > $C/build.txt 2>&1 || { echo "page build failed: $(tail -n 1 $C/build.txt)"; exit 2; }
    { echo "$wait_js"; printf 'const r = await window.rustyac.frameProof(%s, %s, "%s", 0, 0);\nreturn { ...r, picture: (window.rustyac.notes || "").includes("Full mode could not start") ? "the debug view" : "Full mode" };\n' "$(js $C/race.ini)" "$(js $C/assists.ini)" "$(base64 -w0 $C/qr.ryin)"; } > $C/physics.js
    rm -f $C/page_race_out.json; page physics "seed=$SEED&backend=full" $C/physics.js
    node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); const s = r.script || {}; require("fs").writeFileSync(process.argv[2], s.race_out || ""); console.log(`page: step ${s.step} chain ${s.chain} ${(s.fps||0).toFixed(1)} fps, error ${r.error}, ${s.picture}`);' $C/physics.json $C/page_race_out.json
    cmp -s $C/page_race_out.json $C/native/race_out.json && echo "race_out.json: page == native" || echo "race_out.json: page DIFFERS from native"
    ;;
frame)
    { echo "$wait_js"; printf 'await window.rustyac.driveRace(%s, %s);\nreturn JSON.stringify(await window.rustyac.fullProof());\n' "$(js $C/race.ini)" "$(js $C/assists.ini)"; } > $C/frame.js
    page frame "seed=$SEED&backend=full&fullproof=3" $C/frame.js
    size=$(python - $C <<'EOF'
import json, sys
c = sys.argv[1]
r = json.load(open(f'{c}/frame.json'))
s = json.loads(r['script'])
log = s['log'] if s['log'].endswith('\n') else s['log'] + '\n'
open(f'{c}/frames/page.kgllog', 'w', newline='\n').write(log)
print(s['size'].replace(' ', 'x'))
EOF
)
    echo "frame: page size $size, $(wc -l < $C/frames/page.kgllog) log lines, console $(python -c "import json;print(json.load(open('$C/frame.json')).get('console'))")"
    cargo build --release --locked --quiet -p rustyac-web --example full_frame || exit 2
    CARGO_TARGET_DIR=target/full_wgpu cargo build --release --locked --quiet -p rustyac-web --example full_frame --features wgpu || exit 2
    rm -rf target/release/examples/rustyac_cache target/full_wgpu/release/examples/rustyac_cache
    target/release/examples/full_frame.exe --race $C/race.ini --seed $SEED --size $size --frames 3 --warp --out $C/frames --name d3d_warp 2>&1 | tail -n 1 | cut -c1-200
    VK_DRIVER_FILES="$(cygpath -w "$PWD/re/scratch/task38b/swiftshader/vk_swiftshader_icd.json")" RUSTYAC_WGPU_ADAPTER=swiftshader target/full_wgpu/release/examples/full_frame.exe --race $C/race.ini --seed $SEED --size $size --frames 3 --out $C/frames --name wgpu_swift 2>&1 | tail -n 1 | cut -c1-200
    # the same drive with Task 37c's race.ini (sun -48, 4_mid_clear, 26/34 C): its frame must differ
    VK_DRIVER_FILES="$(cygpath -w "$PWD/re/scratch/task38b/swiftshader/vk_swiftshader_icd.json")" RUSTYAC_WGPU_ADAPTER=swiftshader target/full_wgpu/release/examples/full_frame.exe --race re/gold/ai_page/race.ini --seed $SEED --size $size --frames 3 --out $C/frames --name gold_swift 2>&1 | tail -n 1 | cut -c1-200
    rm -rf target/release/examples/rustyac_cache target/full_wgpu/release/examples/rustyac_cache
    cmp -s $C/frames/gold_swift.kgllog $C/frames/wgpu_swift.kgllog && echo "frame: race.ini made NO difference" || echo "frame: Task 37c's race.ini frame differs from this one ($(diff $C/frames/gold_swift.kgllog $C/frames/wgpu_swift.kgllog | grep -c '^<') lines)"
    for ref in wgpu_swift d3d_warp; do
        cmp -s $C/frames/page.kgllog $C/frames/$ref.kgllog && echo "frame: page log == $ref log ($(wc -l < $C/frames/$ref.kgllog) lines)" || echo "frame: page log DIFFERS from $ref log"
    done
    ;;
esac; done
