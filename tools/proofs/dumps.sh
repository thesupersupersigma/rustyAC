#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Physics dumps against the last release: every recorded drive of oracle/game replayed headless with
# --dump-states by the previous version's rustyac.exe and by this one, byte for byte (sha256).
#   DUMPS_TAG=<tag> (default: the newest v* tag that is not HEAD) or DUMPS_OLD=<folder of the old rustyac.exe>
#   DUMPS_EVERY=<n>: every n-th drive (default 1: all of them)
# The old version is built from a git worktree into target/proofs/dumps_old, deleted at the end.
area=dumps
. "$(dirname "$0")/lib.sh"
cargo build --release --locked --quiet -p rustyac-game --bin rustyac || exit 2
old=$DUMPS_OLD
if [ -z "$old" ]; then
    tag=${DUMPS_TAG:-$(git tag --list 'v*' --sort=-v:refname --no-contains HEAD | head -n 1)}
    # the old exe is built once per tag and kept (only the exe and its DLLs) in target/proofs/old_exe/<tag>
    old=$repo/target/proofs/old_exe/$tag
    if [ ! -f "$old/rustyac.exe" ]; then
        src=$repo/target/proofs/dumps_old/src
        rm -rf "$repo/target/proofs/dumps_old"; git worktree prune
        git worktree add --detach "$src" "$tag" > "$out/worktree.txt" 2>&1 || { echo "no worktree of $tag" >&2; exit 2; }
        (cd "$src" && CARGO_TARGET_DIR=$repo/target/proofs/dumps_old/tgt cargo build --release --locked --quiet -p rustyac-game --bin rustyac) || exit 2
        mkdir -p "$old.part" && cp "$repo"/target/proofs/dumps_old/tgt/release/*.exe "$repo"/target/proofs/dumps_old/tgt/release/*.dll "$old.part"/ 2>/dev/null
        mv "$old.part" "$old"
        git worktree remove --force "$src"; rm -rf "$repo/target/proofs/dumps_old"
    fi
    echo "old: $tag" >> "$log.info"
fi
ls oracle/game/*.ryin | awk -v n="${DUMPS_EVERY:-1}" 'NR % n == 1 || n == 1' | while read -r f; do
    keep "$f" || continue  # quick keys: oracle/game/<drive>.ryin
    a=$("$old/rustyac.exe" --replay "$f" --headless --dump-states - 2>/dev/null | sha256sum | cut -c1-16)
    b=$(target/release/rustyac.exe --replay "$f" --headless --dump-states - 2>/dev/null | sha256sum | cut -c1-16)
    if [ "$a" = e3b0c44298fc1c14 ]; then echo "$f: the old exe dumps nothing" >> "$out/left_out.txt"
    elif [ "$a" = "$b" ]; then verdict "$f" IDENTICAL "$a"
    else verdict "$f" DIFFERENT "old $a new $b"; fi
done
summary
