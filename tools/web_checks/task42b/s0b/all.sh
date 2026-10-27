#!/bin/bash
# Task 42b step 0b: the before/after table. Each page (three older releases, v0.42.0 + 42b's
# fixes so far = "now", and the HUD fix = "fix" once built) measured three ways, one at a
# time: the launcher idle, a Debug drive, a Full drive (v0.37.2 has no Full mode).
#   sh tools/web_checks/task42b/s0b/all.sh <label=site folder> ...
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
S=re/scratch/task42b/s0b
for pair in "$@"; do
    label=${pair%%=*}; site=${pair#*=}
    sh tools/web_checks/task42b/s0b/perf.sh ${label}_idle "$site" "" idle | tail -1
    sh tools/web_checks/task42b/s0b/perf.sh ${label}_debug "$site" "backend=auto" drive | tail -1
    [ "$label" = v0.37.2 ] || sh tools/web_checks/task42b/s0b/perf.sh ${label}_full "$site" "backend=full" drive | tail -1
done
