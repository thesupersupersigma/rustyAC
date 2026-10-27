#!/bin/bash
# Task 40: one page drive (copy of task38i/page1.sh) in headless Chrome.
# usage: page1.sh <name> <query> [check.mjs args...]
#   env FLAGS (Chrome flags, default SwiftShader WebGPU), PORT, PROFILE (Chrome profile folder),
#   PACK=<folder>: the drive comes from that pack (tools/web_pack); else the install is served
#   read-only as the test folder /ac-test/
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
S=re/scratch/task40
name=$1; q=$2; shift 2
P=${PORT:-8141}
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
if [ -n "$PACK" ]; then
    py -3 web/serve.py --port $P --pack "$PACK" --quiet > $S/page/$name.serve.log 2>&1 &
    url="http://127.0.0.1:$P/?go=1&$q"
else
    py -3 web/serve.py --port $P --ac "$AC" --quiet > $S/page/$name.serve.log 2>&1 &
    url="http://127.0.0.1:$P/?testfolder=/ac-test/&go=1&$q"
fi
SP=$!
sleep 4
node web/check.mjs --url "$url" --screenshot $S/page/$name.png --profile ${PROFILE:-$S/chrome_profile} --port $((P+1200)) --flags "${FLAGS:---use-webgpu-adapter=swiftshader}" "$@" > $S/page/$name.json 2> $S/page/$name.err
echo "exit $?"
kill $SP 2>/dev/null
