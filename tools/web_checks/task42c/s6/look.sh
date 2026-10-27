#!/bin/bash
# Task 42c step 6: Z4 at Laguna's pit box in Full mode, from the scratch preview pack and from the install
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
S=re/scratch/task42c/s6; P=8193
q="car=bmw_z4_gt3&track=ks_laguna_seca&spawn=pit&backend=full&launcher=0&go=1"
for name in pack install; do
    if [ $name = pack ]; then py -3 web/serve.py --port $P --pack $S/pack --quiet > $S/$name.serve.log 2>&1 & url="http://127.0.0.1:$P/?$q";
    else py -3 web/serve.py --port $P --pack $S/none --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $S/$name.serve.log 2>&1 & url="http://127.0.0.1:$P/?testfolder=/ac-test/&$q"; fi
    sp=$!; sleep 4
    node web/check.mjs --url "$url" --flow $S/${name}_flow.json --profile $S/prof --port 9374 --timeout 400 --flags "${FLAGS:---enable-features=Vulkan}" > $S/$name.json 2> $S/$name.err
    echo "$name: check.mjs exit $?"
    kill $sp 2>/dev/null; wait $sp 2>/dev/null; rm -rf $S/prof
done
