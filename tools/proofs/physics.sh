#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Every saved game recording of the physics oracles, replayed against today's port:
#   oracle/car*/      one car, chassis_compare run --dir (Tasks 9-22)
#   oracle/**/*.mcrec several cars, car_oracle multi-check at 0, 1 and 8 threads (Tasks 24-34)
# The recordings are the durable ones in the ignored oracle/ folder; nothing comes from re/scratch.
area=physics
. "$(dirname "$0")/lib.sh"
tool chassis_compare
tool car_oracle
need oracle/car "the Task 9 recordings"

for dir in oracle/car*/; do
    dir=${dir%/}
    "$CC" run --dir "$dir" > "$out/$(basename "$dir").txt" 2>&1
    # "<scenario>: <n> steps" then "  bit-exact: 100 % (n/n)" or "first difference ..."
    awk -v d="$dir" '
        /^[a-z_0-9]+: [0-9]+ steps/ { s = $1; sub(":", "", s) }
        /bit-exact: 100 %/ && s != "" { print "IDENTICAL " d "/" s; s = "" }
        (/bit-exact: [0-9.]+ %/ && !/100 %/ || /first difference/) && s != "" { print "DIFFERENT " d "/" s; s = "" }
    ' "$out/$(basename "$dir").txt" | tee -a "$log"
done

find oracle -name '*.mcrec' | sort | while read -r rec; do
    name=$(echo "$rec" | tr '/' '_')
    # not proofs: a recording the game made while the port was still wrong (its own saved result
    # says DIFFERENT), and Task 27's version-6 line, whose made-up file was lost (fresh.sh re-records it)
    if grep -q 'DIFFERENT' "${rec%.mcrec}.result.txt" 2>/dev/null; then echo "$rec: recorded as DIFFERENT (a development recording)" >> "$out/left_out.txt"; continue; fi
    case "$rec" in */v6/*|*/v6_line/*) echo "$rec: its AI line file is lost; see fresh.sh" >> "$out/left_out.txt"; continue ;; esac
    "$CO" multi-check "$rec" --threads 0,1,8 > "$out/$name.txt" 2>&1
    if grep -q 'IDENTICAL' "$out/$name.txt" && ! grep -q 'DIFFERENT' "$out/$name.txt"; then
        verdict "$rec" IDENTICAL
    else
        verdict "$rec" DIFFERENT "$(grep -m1 -E 'DIFFERENT|error|panic' "$out/$name.txt")"
    fi
done

summary
