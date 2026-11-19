#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The dumps area's old release: prints the folder of that release's rustyac.exe (and its DLLs),
# target/proofs/old_exe/<tag>, built once per tag from a git worktree (dumps.sh, and prebuild.sh for
# the proof farm, so a new tag on master needs nothing done by hand).
#   bash tools/proofs/old_exe.sh [<tag>]   (default: the newest v* tag that is not HEAD)
repo=$(cd "$(dirname "$0")/../.." && pwd); cd "$repo" || exit 2
tag=${1:-$(git tag --list 'v*' --sort=-v:refname --no-contains HEAD | head -n 1)}
[ -n "$tag" ] || { echo "no release tag to hold this version against" >&2; exit 2; }
old=$repo/target/proofs/old_exe/$tag
if [ ! -f "$old/rustyac.exe" ]; then
    work=$repo/target/proofs/dumps_old
    src=$work/src
    rm -rf "$work"; git worktree prune
    git worktree add --detach "$src" "$tag" >&2 || { echo "no worktree of $tag" >&2; exit 2; }
    (cd "$src" && CARGO_TARGET_DIR=$work/tgt cargo build --release --locked --quiet -p rustyac-game --bin rustyac) || exit 2
    mkdir -p "$old.part" && cp "$work"/tgt/release/*.exe "$work"/tgt/release/*.dll "$old.part"/ 2>/dev/null
    mv "$old.part" "$old"
    git worktree remove --force "$src"; rm -rf "$work"
fi
echo "$old"
