#!/bin/bash
# Task 46c: the page's camera keys and pointer-locked look (look_flow.py) on the fake root.
cd "$(dirname "$0")/../../.." || exit 1
powershell -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w re/scratch/task46c/fakeac.ps1)"
S=re/scratch/task46c/page; mkdir -p $S
P=${PORT:-8172}
python tools/web_checks/task46c/look_flow.py $S/look_flow.json
py -3 web/serve.py --port $P --ac "$(cygpath -w re/scratch/task46c/fakeac)" --quiet > $S/serve.log 2>&1 &
sp=$!
sleep 5
node web/check.mjs --url "http://127.0.0.1:$P/?testfolder=/ac-test/&backend=full" --flow "$(cygpath -am $S/look_flow.json)" --profile "$(cygpath -am $S/chrome_profile)" --port $((P+1200)) --timeout 900 --flags "${FLAGS:---enable-features=Vulkan}" > $S/look.json 2> $S/look.err
echo "check.mjs exit $?"
kill $sp 2>/dev/null
rm -rf $S/chrome_profile
