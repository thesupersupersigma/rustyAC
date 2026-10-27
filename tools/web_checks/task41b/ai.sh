#!/bin/bash
# Task 41b A.1: an AI race's sound (4 cars, 60 s at Laguna Seca, the desktop's tape ai/race.ryin)
# in the page (race_node.mjs: wasm + FMOD 2.03 HTML5 in node) against the desktop's
# `--replay --headless --audio-wav --audio-log`, plain and with the page's answers scripted in.
# Needs target/release/rustyac.exe and the site in re/scratch/task41b/site.
cd "$(dirname "$0")/../../.."
d=re/scratch/task41b/ai
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
desk() { target/release/rustyac.exe --replay $d/race.ryin --headless --audio-wav $d/w.wav --audio-log "$1" --audio-ini $d/audio.ini --cfg-dir $d/empty_cfg --documents $d/empty_docs --camera "$2" --focus "$3" --no-steam "${@:4}" 2>&1 | grep "audio log"; }
for run in "cockpit 0" "chase 0" "chase 2"; do
    set -- $run
    n=${1}_$2
    page=$(node --max-old-space-size=8000 tools/web_checks/task41b/race_node.mjs re/scratch/task41b/site "$AC" $d/race.ini $d/assists.ini 28005 $d/race.ryin $1 $2 $d/page_$n.log < /dev/null 2>&1 | tail -1)
    desk $d/desk_$n.log $1 $2 > /dev/null
    desk $d/desk_${n}_s.log $1 $2 --audio-script $d/page_$n.log > /dev/null
    echo "$1 focus $2 | $page | desktop: $(py tools/proofs/logdiff.py $d/desk_$n.log $d/page_$n.log | head -1 | cut -c1-90) | desktop with the page's answers: $(py tools/proofs/logdiff.py $d/desk_${n}_s.log $d/page_$n.log | head -1)"
done
rm -f $d/w.wav
