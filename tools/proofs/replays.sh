#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Replays, ghost car and comparative telemetry (Task 28c):
#   1 the sessions of tools/proofs/replay_list.txt, the game's recorders beside the port's
#   2 a player's own replays (the kept copy re/gold/docs/replay/temp, never written)
#     in the game's loader and the port's; files the game's own loader crashes on are listed, not cases
#   3 rustyac.exe: a drive recorded, replayed with 0, 1 and 8 workers, one state hash
area=replays
. "$(dirname "$0")/lib.sh"
tool car_oracle
root=$out/root

multi_list tools/proofs/replay_list.txt "$root" "$out/rec"

tracks=$(ls "$ac/content/tracks")
for f in "$docs_gold/replay/temp/"*.acreplay; do
    [ -f "$f" ] || continue
    b=$(basename "$f" .acreplay)
    keep "replay-file $b" || continue  # quick keys: "multi ...", "replay-file <name>"
    # the name ends _<track>_<layout>; the longest installed track folder in it wins
    t=$(for x in $tracks; do case "$b" in *"_${x}_"*) echo "${#x} $x" ;; esac; done | sort -rn | head -n 1 | cut -d' ' -f2)
    [ -n "$t" ] || { skip "replay-file $b" "its track is not installed"; continue; }
    l=${b##*_"$t"_}
    "$CO" replay-file "$f" --track "$t" ${l:+--layout "$l"} --root "$root" > "$out/file.txt" 2>&1 < /dev/null
    r=$(grep -m1 '^replay-file |' "$out/file.txt")
    case "$r" in
        *"| IDENTICAL") verdict "replay-file $b" IDENTICAL ;;
        *) if grep -q "game's code crashed" "$out/file.txt"; then skip "replay-file $b" "the game's own loader crashes on it (docs/port/replays.md)"
           else verdict "replay-file $b" DIFFERENT "$(grep -m1 -E 'DIFFERENT|differ|rror' "$out/file.txt")"; fi ;;
    esac
done

build rustyac cargo build --release --locked --quiet -p rustyac-game --bin rustyac
exe=target/release/rustyac.exe
rustyac $exe --car ks_ferrari_f2004 --track spa --no-race-ini --spawn start --autodrive --headless --duration 60 --no-audio --record "$out/spa.ryin" > "$out/record.txt" 2>&1
hashes=$(for w in 0 1 8; do rustyac $exe --replay "$out/spa.ryin" --headless --state-hash --physics-threads $w 2>&1 | grep -oE "state hash [0-9a-f]+"; done | sort -u)
[ "$(echo "$hashes" | wc -l)" = 1 ] && [ -n "$hashes" ] && verdict "rustyac.exe replay at 0, 1, 8 workers" IDENTICAL "$hashes" || verdict "rustyac.exe replay at 0, 1, 8 workers" DIFFERENT "$hashes"
rm -f "$out/spa.ryin"

rm -rf "$root"
summary
