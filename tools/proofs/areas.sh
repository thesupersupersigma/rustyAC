#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The proof areas a list of changed files asks for (tools/proofs/areas.map): file names on stdin,
# one a line; why each area was picked on stderr, the areas on stdout (one line, run_all.sh's order).
#   git diff --name-only v0.38.3 | PROOF_SINCE=v0.38.3 bash tools/proofs/areas.sh
# With PROOF_SINCE=<tag>, a Cargo.toml or Cargo.lock whose only change since <tag> is rustyAC's own
# version (every release bumps it in every lock file) asks for nothing.
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
while read -r f; do
    [ -n "$f" ] || continue
    if version_only "$f"; then echo "$f: only rustyAC's version ($old to $new): none" >&2; continue; fi
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
