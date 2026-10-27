#!/bin/bash
# Task 42b: one --flow page run against the install. usage: flow.sh <name> <query> <flow.json>
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
S=re/scratch/task42b/s0; name=$1; q=$2; flow=$3
P=${PORT:-8161}
py -3 web/serve.py --port $P --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $S/$name.serve.log 2>&1 &
sp=$!
sleep 5
node web/check.mjs --url "http://127.0.0.1:$P/?testfolder=/ac-test/&$q" --flow "$flow" --profile $S/chrome_profile --port $((P+1200)) --timeout 1200 --flags "${FLAGS:---enable-features=Vulkan}" > $S/$name.json 2> $S/$name.err
echo "$name: check.mjs exit $?"
kill $sp 2>/dev/null
rm -rf $S/chrome_profile
