#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Makes re/gold/launcher/<name>.req again: each click script of tools/proofs/launcher/ drives the
# port's launcher window (off screen, the theme of the AC install, rustyac.exe headless with the AI
# driving) from an empty player folder; log2req.py turns the run's log into the request list.
# The hand-written lists (tools/proofs/launcher/*.req) are copied as they are.
#   sh tools/proofs/launcher_record.sh [name ...]
area=launcher_record
. "$(dirname "$0")/lib.sh"
cargo build --release --locked --quiet -p rustyac-launcher -p rustyac-game || exit 2
mkdir -p "$gold/launcher"
cp tools/proofs/launcher/*.req "$gold/launcher/"
names=${*:-$(cd tools/proofs/launcher && ls *.txt | sed 's/\.txt$//')}
for name in $names; do
    docs=$out/$name/docs
    rm -rf "$out/$name"; mkdir -p "$docs" "$out/$name/out"
    extra=$(sed -n '1s/^# game args[^:]*: *//p' "tools/proofs/launcher/$name.txt")
    set --
    for a in --headless --no-steam --autodrive --duration 160 $extra; do set -- "$@" --game-arg "$a"; done
    # the script's shots land in out/ of the cwd
    (cd "$out/$name" && "$repo/target/release/rustyac-launcher.exe" --docs "$docs" "$@" \
        --script "$repo/tools/proofs/launcher/$name.txt" --log run.log --offscreen 1600x900) > "$out/$name/launcher.txt" 2>&1
    status=$?
    n=$(python tools/proofs/log2req.py "$out/$name/run.log" "$gold/launcher/$name.req")
    fails=$(grep -cE '^FAIL|nothing matches' "$out/$name/run.log")
    echo "$name: exit $status, $n, $fails failed steps" | tee -a "$log"
    # the window's browser lets go of its folder a moment after the window closes
    for _ in 1 2 3 4 5; do rm -rf "$docs" 2> /dev/null && break; sleep 2; done
done
