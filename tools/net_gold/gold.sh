#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Turns the five tapes of record.sh into the 20 multiplayer goldens: the game's own client
# (car_oracle net) replays each tape in four scenarios and writes what it did at every step.
# Needs tools/car_oracle built (cargo build --release in tools/car_oracle) and the AC folder.
#
#   sh tools/net_gold/gold.sh            -> re/gold/net/<tape>_<scenario>.netgold, gold.log
set -eu
repo=$(cd "$(dirname "$0")/../.." && pwd)
gold=$repo/re/gold/net
oracle=$repo/tools/car_oracle/target/release/car_oracle.exe
: > "$gold/gold.log"
failed=0
for tape in laguna_a laguna_b rich_a rich_b rich_c; do
    case $tape in laguna_*) steps=66000 ;; *) steps=50000 ;; esac
    for scenario in net_watch net_block net_drive net_pit; do
        # --count: the golden holds every step, not the default first 1,500
        all=$("$oracle" net "$gold/tapes/$tape.nettape" --scenario $scenario --track ks_laguna_seca \
            --steps $steps --count $steps --csv-out "$gold/${tape}_$scenario.netgold" 2>&1) || true
        # the verdict, then the line about the golden written
        line=$(echo "$all" | grep -E "IDENTICAL|DIFFERENT" | tail -1)
        echo "$tape $line" | tee -a "$gold/gold.log"
        echo "$all" | grep "wrote the golden" >> "$gold/gold.log" || true
        case $line in *IDENTICAL*) ;; *) echo "the game's client did not replay $tape in $scenario" >&2; failed=$((failed + 1)) ;; esac
    done
done
echo "failed: $failed" | tee -a "$gold/gold.log"
[ $failed = 0 ]
