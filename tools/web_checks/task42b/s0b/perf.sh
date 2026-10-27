#!/bin/bash
# Task 42b step 0b: one measured page run. usage: perf.sh <name> <site folder> <query> idle|drive
# env FLAGS (Chrome flags, default the real GPU), PORT
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
S=re/scratch/task42b/s0b; name=$1; site=$2; q=$3; what=$4
P=${PORT:-8171}
python tools/web_checks/task42b/s0b/mkperf.py $name $what
py -3 web/serve.py --site "$site" --port $P --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $S/$name.serve.log 2>&1 &
sp=$!
sleep 5
node web/check.mjs --url "http://127.0.0.1:$P/?testfolder=/ac-test/&$q" --flow $S/${name}_flow.json --profile $S/chrome_profile --port $((P+1200)) --timeout 900 --flags "${FLAGS:---enable-features=Vulkan}" > $S/$name.json 2> $S/$name.err
echo "$name: check.mjs exit $?"
kill $sp 2>/dev/null
rm -rf $S/chrome_profile
python tools/web_checks/task42b/s0b/sum.py $S/$name.json
