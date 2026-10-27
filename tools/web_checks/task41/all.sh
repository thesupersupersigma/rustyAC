#!/bin/bash
# all.sh [filter]: the 18 drives of tools/proofs/sound.sh, desktop port (FMOD 1.08) against the page (node, FMOD 2.03 HTML5)
cd "$(dirname "$0")/../../.."
cp web/sound.js re/scratch/task41/site/sound.js
AO=tools/audio_oracle/target/release/audio_oracle.exe
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
D=re/scratch/task41
sed -n '27,44p' tools/proofs/sound.sh | grep "${1:-.}" | while read -r car scen cam; do
  n=${car}_${scen}_${cam}
  [ -f $D/desk/${n}_port.log ] || $AO run --side port --tape oracle/audio/$car/$scen.audiotape --camera $cam --out $D/desk --root $D/desk/root > $D/desk/$n.txt 2>&1 < /dev/null
  t0=$(date +%s.%N)
  node tools/web_checks/task41/node_proof.mjs $D/site "$AC" $car - oracle/audio/$car/$scen.audiotape $cam $D/page/$n.log $D/page/$n.wav > $D/page/$n.txt 2>&1 < /dev/null
  r=$(python tools/proofs/logdiff.py $D/desk/${n}_port.log $D/page/$n.log 2>&1 | head -1 | cut -c1-90)
  echo "$car $scen $cam | $(grep -m1 '^sound_proof' $D/page/$n.txt || grep -m1 Error $D/page/$n.txt | cut -c1-120) | $r"
done
