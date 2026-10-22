#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The proof areas a list of changed files asks for (tools/proofs/areas.map): file names on stdin,
# one a line; why each area was picked on stderr, the areas on stdout (one line, run_all.sh's order).
#   git diff --name-only v0.38.3 | bash tools/proofs/areas.sh
all="physics fresh sound render gui replays net browser ai_page shaders launcher dumps"
map=$(dirname "$0")/areas.map
rules=$(grep -vE '^\s*(#|$)' "$map")
picked=" "
while read -r f; do
    [ -n "$f" ] || continue
    hit=
    while read -r glob areas; do
        # shellcheck disable=SC2053
        if [[ $f == $glob ]]; then hit=$areas; break; fi
    done <<< "$rules"
    if [ -z "$hit" ]; then echo "$f: no rule, so every area" >&2; hit=$all
    elif [ "$hit" = none ]; then echo "$f: none" >&2; continue
    else echo "$f ($glob): $hit" >&2; fi
    for a in $hit; do picked="$picked$a "; done
done
out=
for a in $all; do case "$picked" in *" $a "*) out="$out $a" ;; esac; done
echo "${out# }"
