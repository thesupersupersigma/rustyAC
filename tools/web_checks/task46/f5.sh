#!/bin/bash
# Task 46b: F5 in the page (Full mode) (f5_flow.py), against the install.
cd "$(dirname "$0")/../../.." || exit 1
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
S=re/scratch/task46b/page; mkdir -p $S
P=${PORT:-8162}
python tools/web_checks/task46/f5_flow.py $S/f5_flow.json
py -3 web/serve.py --port $P --ac "$AC" --quiet > $S/serve.log 2>&1 &
sp=$!
sleep 5
node web/check.mjs --url "http://127.0.0.1:$P/?testfolder=/ac-test/&backend=full" --flow "$(cygpath -am $S/f5_flow.json)" --profile "$(cygpath -am $S/chrome_profile)" --port $((P+1200)) --timeout 900 --flags "${FLAGS:---enable-features=Vulkan}" > $S/f5.json 2> $S/f5.err
echo "check.mjs exit $?"
kill $sp 2>/dev/null
rm -rf $S/chrome_profile
