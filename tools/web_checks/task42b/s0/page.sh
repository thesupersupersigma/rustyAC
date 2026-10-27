#!/bin/bash
# Task 42b: one page run against the install. usage: page.sh <name> <query> <script.js> [check.mjs args]
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
S=re/scratch/task42b/s0; name=$1; q=$2; script=$3; shift 3
P=${PORT:-8161}
py -3 web/serve.py --port $P --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $S/$name.serve.log 2>&1 &
sp=$!
sleep 5
node web/check.mjs --url "http://127.0.0.1:$P/?testfolder=/ac-test/&$q" --menu --script "$script" --screenshot $S/$name.png --profile $S/chrome_profile --port $((P+1200)) --timeout 1200 --flags "${FLAGS:---use-webgpu-adapter=swiftshader}" "$@" > $S/$name.json 2> $S/$name.err
echo "$name: check.mjs exit $?"
kill $sp 2>/dev/null
rm -rf $S/chrome_profile
