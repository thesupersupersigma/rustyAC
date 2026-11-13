#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The render oracle (Tasks 20-23, Y2c): every frame and sequence of tools/proofs/render_list.txt drawn by
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
    # Task Y5: RESIZE:<frame>:<W>x<H>, GLARE:<0-5>, PP_QUALITY:<0-5>, RAYS:1 (Y5d) -> RENDER_ORACLE_<KEY> (see render_list.txt)
    for key in RESIZE GLARE PP_QUALITY RAYS; do
        v=$(echo "$args" | grep -oE "(^| )$key:[^ ]+" | tr -d ' ')
        if [ -n "$v" ]; then export RENDER_ORACLE_$key=${v#$key:}; args=$(echo "$args" | sed "s#$v##"); else unset RENDER_ORACLE_$key; fi
    done
    # Task Y2c: a --post row is the real YEBIS on both sides (an empty value would still count as on)
    # Task Y3d-2: and from frame 0, where YEBIS makes its programs and wraps the scene's views
    case " $args " in *" --post "*) export RENDER_ORACLE_YEBIS=1; args="$args --capture 0" ;; *) unset RENDER_ORACLE_YEBIS ;; esac
    room
    for try in 1 2; do  # a second try only for a log that differs while the pictures agree (load-flaky)
        "$RO" compare $args --label "$name" --root "$root" --out "$out/frames" > "$out/$name.txt" 2>&1 < /dev/null
        grep -q '^command log: DIFFERENT' "$out/$name.txt" && grep -q '^pixels: IDENTICAL' "$out/$name.txt" || break
    done
    detail=$(grep -m1 -E '^frame |frames:' "$out/$name.txt")
    if grep -q '^command log: IDENTICAL' "$out/$name.txt" && grep -q '^pixels: IDENTICAL' "$out/$name.txt" && ! grep -q 'DIFFERENT' "$out/$name.txt"; then
        verdict "$name" IDENTICAL "$detail"
    else
        verdict "$name" DIFFERENT "$(why "$out/$name.txt" 'DIFFERENT|rror|panic')"
    fi
    rm -rf "$out/frames"
done

# Task 45b: every layout of the install without data/overlays.ini: the game's own
# createAndSaveOverlays (into $out/ov, not the install) against the port's text, byte for byte,
# then the frame drawn by both (the gate view where there is a gate)
for t in "$ac"/content/tracks/*/; do
    t=${t%/}; n=$(basename "$t"); layouts=""
    for l in "$t"/*/; do l=${l%/}; [ -f "$l/data/surfaces.ini" ] && layouts="$layouts $(basename "$l")"; done
    for l in ${layouts:--}; do
        if [ "$l" = "-" ]; then data=$t; args="--track $n"; key=$n; else data=$t/$l; args="--track $n --layout $l"; key=$n/$l; fi
        [ -f "$data/data/overlays.ini" ] || [ ! -f "$data/ai/fast_lane.ai" ] && continue
        name="overlays ${key}"; keep "$name" || continue
        file=$out/ov_${key//\//_}.txt
        # (no AC_TIME_0_L: no gate to look at, the start's view)
        view="--view gate"; grep -qa "AC_TIME_0_L" "$data"/../*.kn5 "$t"/*.kn5 2>/dev/null || view=""
        # shellcheck disable=SC2086
        RENDER_ORACLE_OVERLAYS="$out/ov/${key//\//_}" "$RO" compare $args $view --frames 1 --set gate=0 --label "ov_${key//\//_}" --root "$root" --out "$out/frames" > "$file" 2>&1 < /dev/null
        if grep -q '^overlays .* IDENTICAL$' "$file" && grep -q '^command log: IDENTICAL' "$file" && grep -q '^pixels: IDENTICAL' "$file"; then
            verdict "$name" IDENTICAL "$(grep -m1 '^overlays' "$file" | sed 's/^overlays [^:]*: //')"
        else
            verdict "$name" DIFFERENT "$(why "$file" '^overlays|DIFFERENT|rror|panic|failed')"
        fi
        rm -rf "$out/frames"
    done
done

rm -rf "$root"
summary
