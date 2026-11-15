#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The YEBIS settings sweep (Task Y7d): the rows of yebis_sweep.py (every PP quality x glare x DOF x
# light shafts, pairwise coverage with heat shimmer / FXAA / AA / triple screens at two more sizes,
# every shipped filter), game against port on WARP like render.sh: call log and pixels. Full runs only
# (no quick list). A failing case's pictures stay in <out>/fail/<case>; passing ones are deleted.
#   SWEEP_ONLY=<regex> runs only the matching case names.
area=yebis_sweep
. "$(dirname "$0")/lib.sh"
tool render_oracle
root=$out/root

python "$repo/tools/proofs/yebis_sweep.py" "$ac" > "$out/rows.txt" || { echo "yebis_sweep.py failed" >&2; exit 2; }
grep -E "^(${SWEEP_ONLY:-.*}) " "$out/rows.txt" | while IFS='|' read -r name args; do
    name=$(echo "$name" | tr -d ' ')
    keep "$name" || continue
    for key in GLARE PP_QUALITY RAYS DOF HEAT_SHIMMER; do
        v=$(echo "$args" | grep -oE "(^| )$key:[^ ]+" | tr -d ' ')
        if [ -n "$v" ]; then export RENDER_ORACLE_$key=${v#$key:}; args=$(echo "$args" | sed "s#$v##"); else unset RENDER_ORACLE_$key; fi
    done
    export RENDER_ORACLE_YEBIS=1
    room
    for try in 1 2; do  # a second try only for a log that differs while the pictures agree (load-flaky)
        # shellcheck disable=SC2086
        "$RO" compare $args --capture 0 --label "$name" --root "$root" --out "$out/frames" > "$out/$name.txt" 2>&1 < /dev/null
        grep -q '^command log: DIFFERENT' "$out/$name.txt" && grep -q '^pixels: IDENTICAL' "$out/$name.txt" || break
    done
    if grep -q '^command log: IDENTICAL' "$out/$name.txt" && grep -q '^pixels: IDENTICAL' "$out/$name.txt" && ! grep -q 'DIFFERENT' "$out/$name.txt"; then
        verdict "$name" IDENTICAL "$(grep -m1 -E '^frame |frames:' "$out/$name.txt")"
        rm -f "$out/$name.txt"
    else
        verdict "$name" DIFFERENT "$(why "$out/$name.txt" 'DIFFERENT|rror|panic')"
        mkdir -p "$out/fail" && mv "$out/frames" "$out/fail/$name"
    fi
    rm -rf "$out/frames"
done

rm -rf "$root"
summary
