#!/bin/bash
# Task 42b steps 3-4: the keys proof (see mkflow.py). usage: keys.sh [name]
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
here=re/scratch/task42b/s4; name=${1:-keys}
P=${PORT:-8163}
mkdir -p $here/$name
python tools/web_checks/task42b/s4/mkflow.py $here/$name/flow.json $here/$name
py -3 web/serve.py --port $P --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $here/$name/serve.log 2>&1 &
sp=$!
sleep 5
node web/check.mjs --url "http://127.0.0.1:$P/?testfolder=/ac-test/&backend=full" --flow $here/$name/flow.json --profile $here/prof --port $((P+1200)) --timeout 1200 --flags "${FLAGS:---enable-features=Vulkan}" > $here/$name/report.json 2> $here/$name/err.txt
echo "$name: check.mjs exit $?"
kill $sp 2>/dev/null
rm -rf $here/prof
py -c "
import json
r = json.load(open('$here/$name/report.json'))
for s in r['flow']:
    if 'js' in s or 'failed' in s or 'until' in s: print(s.get('at'), s.get('until') or s.get('failed') or str(s.get('value'))[:260])
print('console', r.get('console'))"
