#!/bin/bash
# run.sh <car> <track> <scenario> <camera> [build]: the page's sound proof in node
cd "$(dirname "$0")/../../.."
export CARGO_BUILD_JOBS=4
[ "$5" = build ] && { python web/build.py --no-opt --out re/scratch/task41/site > re/scratch/task41/build.log 2>&1 || { tail -20 re/scratch/task41/build.log; exit 1; }; }
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
mkdir -p re/scratch/task41/page
node tools/web_checks/task41/node_proof.mjs re/scratch/task41/site "$AC" $1 $2 oracle/audio/$1/$3.audiotape $4 re/scratch/task41/page/$1_$3_$4.log re/scratch/task41/page/$1_$3_$4.wav 2>&1 | grep -v "^var \|^ *$\|readyPromise" | cut -c1-240 | tail -${TAILN:-25}
