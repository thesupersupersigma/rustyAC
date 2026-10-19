#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Every proof of the project, one area or all of them (see tools/proofs/README.md):
#   sh tools/proofs/run_all.sh             all areas, one after the other
#   sh tools/proofs/run_all.sh gui net     only these
# Needs: this repository, the ignored oracle/ and re/gold/ folders, the AC install (read only).
# Each area writes into target/proofs/<area> and ends with "PROOF <area> cases= identical= different=";
# the table below is made from those lines. Exit code 0 only when no area has a difference.
here=$(cd "$(dirname "$0")" && pwd)
all="physics fresh sound render gui replays net browser ai_page shaders launcher dumps"
areas=${*:-$all}
out=${PROOF_OUT:-$here/../../target/proofs}
mkdir -p "$out"
table=$out/table.md
echo "| area | cases | identical | different | time |" > "$table"
echo "|---|---|---|---|---|" >> "$table"
bad=0
for a in $areas; do
    [ -f "$here/$a.sh" ] || { echo "no area $a (areas: $all)" >&2; bad=1; continue; }
    start=$(date +%s)
    bash "$here/$a.sh" > "$out/$a.log" 2>&1
    code=$?
    secs=$(( $(date +%s) - start ))
    line=$(grep '^PROOF ' "$out/$a.log" | tail -n 1)
    if [ -n "$line" ]; then
        c=$(echo "$line" | sed -E 's/.*cases=([0-9]+).*/\1/'); i=$(echo "$line" | sed -E 's/.*identical=([0-9]+).*/\1/'); d=$(echo "$line" | sed -E 's/.*different=([0-9]+).*/\1/')
    else
        c=-; i=-; d="did not run ($(tail -n 1 "$out/$a.log"))"
    fi
    [ "$code" = 0 ] || bad=1
    printf '| %s | %s | %s | %s | %dm%02ds |\n' "$a" "$c" "$i" "$d" $((secs / 60)) $((secs % 60)) | tee -a "$table"
done
echo; cat "$table"
exit $bad
