#!/bin/bash
# same41.sh [filter]: Task 41's 18 drives in node with this build (re/scratch/task41b/site): the
# call log against Task 41's page log (must be identical) and against the desktop's
cd "$(dirname "$0")/../../.."
cp web/sound.js re/scratch/task41b/site/sound.js
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
D=re/scratch/task41
O=re/scratch/task41b/page41
mkdir -p $O
sed -n '27,44p' tools/proofs/sound.sh | grep "${1:-.}" | while read -r car scen cam; do
  n=${car}_${scen}_${cam}
  node tools/web_checks/task41/node_proof.mjs re/scratch/task41b/site "$AC" $car - oracle/audio/$car/$scen.audiotape $cam $O/$n.log > $O/$n.txt 2>&1 < /dev/null
  old=$(python tools/proofs/logdiff.py $D/page/$n.log $O/$n.log 2>&1 | head -1 | cut -c1-70)
  desk=$(python tools/proofs/logdiff.py $D/desk/${n}_port.log $O/$n.log 2>&1 | head -1 | cut -c1-70)
  echo "$car $scen $cam | $(grep -m1 '^sound_proof' $O/$n.txt || grep -m1 Error $O/$n.txt | cut -c1-120) | old page: $old | desktop: $desk"
  rm -f $O/$n.log
done
