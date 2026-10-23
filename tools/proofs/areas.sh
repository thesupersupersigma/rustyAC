#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The proof areas a list of changed files asks for (tools/proofs/areas.map): file names on stdin,
# one a line; why each area was picked on stderr, the areas on stdout (one line, run_all.sh's order).
#   git diff --name-only v0.38.3 | PROOF_SINCE=v0.38.3 bash tools/proofs/areas.sh
# With PROOF_SINCE=<tag>, a Cargo.toml or Cargo.lock whose only change since <tag> is rustyAC's own
# version (every release bumps it in every lock file) asks for nothing, and a Cargo.lock no rule
# names asks for what its changed crates' own rules ask for (tools/proofs/lock_crates.py: the path
# crates whose dependencies changed); every area when that can't be told.
all="physics fresh sound render gui replays net browser ai_page shaders launcher dumps"
cd "$(dirname "$0")/../.." || exit 2
map=tools/proofs/areas.map
rules=$(grep -vE '^\s*(#|$)' "$map")
picked=" "
ws_version() {  # ws_version <file content on stdin>: [workspace.package] version
    sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p'
}
if [ -n "$PROOF_SINCE" ]; then
    old=$(git show "$PROOF_SINCE:Cargo.toml" | ws_version)
    new=$(ws_version < Cargo.toml)
fi
# version_only <file>: the file's only change since PROOF_SINCE is rustyAC's version
version_only() {
    [ -n "$PROOF_SINCE" ] && [ -n "$old" ] || return 1
    case "$1" in Cargo.toml|*/Cargo.toml|Cargo.lock|*/Cargo.lock) ;; *) return 1 ;; esac
    ! git diff -U0 "$PROOF_SINCE" -- "$1" | grep -E '^[-+]' | grep -vE '^(\+\+\+|---) ' \
        | grep -vxF -e "-version = \"$old\"" -e "+version = \"$new\"" | grep -q .
}
rule_of() {  # rule_of <file>: the areas of the first rule that matches (glob in $glob), or nothing
    glob=
    while read -r glob areas; do
        # shellcheck disable=SC2053
        if [[ $1 == $glob ]]; then echo "$areas"; return; fi
    done <<< "$rules"
}
# lock_areas <Cargo.lock>: the areas its changed crates ask for; nothing when it can't tell
lock_areas() {
    [ -n "$PROOF_SINCE" ] || { echo "$1: no tag to compare with" >&2; return; }
    crates=$(python tools/proofs/lock_crates.py "$1" "$PROOF_SINCE" | tr -d '\015'; exit "${PIPESTATUS[0]}") || return
    [ -n "$crates" ] || { echo none; return; }
    got=
    for c in $crates; do
        a=$(rule_of "$c/Cargo.toml")
        [ -n "$a" ] || { echo "$1: $c changed and has no rule" >&2; return; }
        echo "$1: $c changed: $a" >&2
        [ "$a" = none ] || got="$got $a"
    done
    echo "${got:- none}"
}
while read -r f; do
    [ -n "$f" ] || continue
    if version_only "$f"; then echo "$f: only rustyAC's version ($old to $new): none" >&2; continue; fi
    hit=$(rule_of "$f")
    if [ -z "$hit" ]; then
        case "$f" in Cargo.lock|*/Cargo.lock) hit=$(lock_areas "$f") ;; esac
    fi
    if [ -z "$hit" ]; then echo "$f: no rule, so every area" >&2; hit=$all
    elif [ "$hit" = none ]; then echo "$f: none" >&2; continue
    else echo "$f: $hit" >&2; fi
    for a in $hit; do picked="$picked$a "; done
done
out=
for a in $all; do case "$picked" in *" $a "*) out="$out $a" ;; esac; done
echo "${out# }"
