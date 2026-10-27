#!/bin/bash
# usage: cost.sh <name> [query]
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
S=re/scratch/task42c/s4; name=$1; q=${2:-backend=full}; P=8177
python tools/web_checks/task42c/s4/mkcost.py $S/${name}_flow.json $S/$name $3
py -3 web/serve.py --port $P --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $S/$name.serve.log 2>&1 &
sp=$!
sleep 5
node web/check.mjs --url "http://127.0.0.1:$P/?testfolder=/ac-test/&$q" --flow $S/${name}_flow.json --launcher --profile $S/chrome_profile --port $((P+1200)) --timeout 600 --flags "${FLAGS:---enable-features=Vulkan}" > $S/$name.json 2> $S/$name.err
echo "$name: check.mjs exit $?"
kill $sp 2>/dev/null
rm -rf $S/chrome_profile
python -c "
import json
r = json.load(open('$S/$name.json'))
for s in r['flow']:
    v = str(s.get('value', ''))
    if 'per_10s' in v or 'ac://' in v: print(v[:1500])
    elif 'error' in str(s).lower() or 'fail' in str(s).lower(): print(str(s)[:300])
"
