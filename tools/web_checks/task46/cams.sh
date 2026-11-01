#!/bin/bash
# Task 46: the page's cockpit keys in Full mode (cams_flow.py), against the install.
cd "$(dirname "$0")/../../.." || exit 1
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
S=re/scratch/task46/page; mkdir -p $S
P=${PORT:-8162}
python tools/web_checks/task46/cams_flow.py $S/cams_flow.json "$AC/cfg/controllers/presets/Default.ini"
py -3 web/serve.py --port $P --ac "$AC" --quiet > $S/serve.log 2>&1 &
sp=$!
sleep 5
node web/check.mjs --url "http://127.0.0.1:$P/?testfolder=/ac-test/&backend=full" --flow "$(cygpath -am $S/cams_flow.json)" --profile "$(cygpath -am $S/chrome_profile)" --port $((P+1200)) --timeout 900 --flags "${FLAGS:---enable-features=Vulkan}" > $S/cams.json 2> $S/cams.err
echo "check.mjs exit $?"
kill $sp 2>/dev/null
rm -rf $S/chrome_profile
