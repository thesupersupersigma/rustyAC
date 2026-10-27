#!/bin/bash
# Task 42b step 0b: the pages of three older releases, each built in its own worktree (one at a
# time, 4 jobs), into re/scratch/task42b/tgt_old/<tag>/site. usage: old.sh [tags...]
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
export CARGO_BUILD_JOBS=4
for t in ${@:-v0.37.2 v0.38.9 v0.40.0}; do
    d=re/scratch/task42b/tgt_old/$t
    (cd $d && py -3 web/build.py --out "$(pwd -W)/site") > re/scratch/task42b/s0b/build_$t.log 2>&1
    echo "$t: exit $? $(tail -1 re/scratch/task42b/s0b/build_$t.log)"
done
