#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The render oracle (Tasks 20-23): every frame and sequence of tools/proofs/render_list.txt drawn by
# the game's renderer and by the port on WARP, compared call by call and byte by byte.
#   RENDER_ONLY=<regex> runs only the matching case names.
# One compare at a time: two at once make the game's side load-flaky.
area=render
. "$(dirname "$0")/lib.sh"
tool render_oracle
root=$out/root

# shellcheck disable=SC2086
grep -vE '^\s*(#|$)' tools/proofs/render_list.txt | grep -E "^(${RENDER_ONLY:-.*}) " | while IFS='|' read -r name args; do
    name=$(echo "$name" | tr -d ' ')
    keep "$name" || continue  # quick keys: the row's name
    # POSE:<tape>:<frame> -> a pose cut from the tape
    for p in $(echo "$args" | grep -oE 'POSE:[^ ]+'); do
        spec=${p#POSE:}; tape=${spec%:*}; frame=${spec##*:}
        pose=$out/poses/$name.pose
        mkdir -p "$out/poses"
        "$RO" pose-of --tape "$tape" --tape-from "$frame" --pose "$pose" > /dev/null 2>&1 < /dev/null
        args=$(echo "$args" | sed "s#$p#$pose#")
    done
    for try in 1 2; do  # a second try only for a log that differs while the pictures agree (load-flaky)
        "$RO" compare $args --label "$name" --root "$root" --out "$out/frames" > "$out/$name.txt" 2>&1 < /dev/null
        grep -q '^command log: DIFFERENT' "$out/$name.txt" && grep -q '^pixels: IDENTICAL' "$out/$name.txt" || break
    done
    detail=$(grep -m1 -E '^frame |frames:' "$out/$name.txt")
    if grep -q '^command log: IDENTICAL' "$out/$name.txt" && grep -q '^pixels: IDENTICAL' "$out/$name.txt" && ! grep -q 'DIFFERENT' "$out/$name.txt"; then
        verdict "$name" IDENTICAL "$detail"
    else
        verdict "$name" DIFFERENT "$(grep -m1 -E 'DIFFERENT|rror|panic' "$out/$name.txt")"
    fi
    rm -rf "$out/frames"
done

rm -rf "$root"
summary
