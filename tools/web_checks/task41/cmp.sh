#!/bin/bash
# cmp.sh <car> <track> <scenario> <camera>: page run in node, then its log against the desktop's
cd "$(dirname "$0")/../../.."
cp web/sound.js re/scratch/task41/site/sound.js
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
n=$1_$3_$4
node tools/web_checks/task41/node_proof.mjs re/scratch/task41/site "$AC" $1 $2 oracle/audio/$1/$3.audiotape $4 re/scratch/task41/page/$n.log re/scratch/task41/page/$n.wav 2>&1 | grep -v "^var \|readyPromise\|^ *$" | cut -c1-200 | grep -v "^    at" | tail -4
python tools/proofs/logdiff.py re/scratch/task41/desk/${n}_port.log re/scratch/task41/page/$n.log 2>&1 | head -${DIFFN:-12} | cut -c1-260
