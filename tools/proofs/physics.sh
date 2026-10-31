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

# quick keys: the folder (oracle/car_wc) or the recording (oracle/t28/jump/rc_jump.mcrec)
for dir in oracle/car*/; do
    dir=${dir%/}
    keep "$dir" || continue
    "$CC" run --dir "$dir" > "$out/$(basename "$dir").txt" 2>&1
    # "<scenario>: <n> steps" then "  bit-exact: 100 % (n/n)" or "first difference ..."
    awk -v d="$dir" '
        /^[a-z_0-9]+: [0-9]+ steps/ { s = $1; sub(":", "", s) }
        /bit-exact: 100 %/ && s != "" { print "IDENTICAL " d "/" s; s = "" }
        (/bit-exact: [0-9.]+ %/ && !/100 %/ || /first difference/) && s != "" { print "DIFFERENT " d "/" s; s = "" }
    ' "$out/$(basename "$dir").txt" | tee -a "$log"
done

find oracle -name '*.mcrec' | sort | while read -r rec; do
    keep "$rec" || continue
    name=$(echo "$rec" | tr '/' '_')
    # not proofs, each listed as skipped: recordings the game made while the port's referee stand-in was
    # still wrong (Task 28's, a deliberate fault of Task 34), and Task 27's version-6 line, whose
    # made-up file was lost (fresh.sh records it again). A fixed list, not the recording's own
    # result.txt: multi-check writes that file, so one bad run would hide the recording for good.
    case "$rec" in
        oracle/t28/cut/*|oracle/t28/race6/*|oracle/t28/race_short/*|oracle/t28/timed/*|oracle/t28/weekend/*|oracle/t28/weekend_dbg/*)
            skip "$rec" "recorded during Task 28 against the old referee stand-in (DIFFERENT at step 2, world.referee.lo)"; continue ;;
        oracle/t34/mp_fault/*) skip "$rec" "a deliberate fault of Task 34 (development recording)"; continue ;;
        */v6/*|*/v6_line/*) skip "$rec" "its AI line file is lost; fresh.sh records it again"; continue ;;
    esac
    "$CO" multi-check "$rec" --threads 0,1,8 > "$out/$name.txt" 2>&1
    if grep -q 'IDENTICAL' "$out/$name.txt" && ! grep -q 'DIFFERENT' "$out/$name.txt"; then
        verdict "$rec" IDENTICAL
    else
        verdict "$rec" DIFFERENT "$(grep -m1 -E 'DIFFERENT|error|panic' "$out/$name.txt")"
    fi
done

summary
