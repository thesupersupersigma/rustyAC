#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Multiplayer (Tasks 25-26, 37): the 20 goldens of re/gold/net checked four ways by
# tools/net_gold/check.sh (desktop, browser, gateway, wasm): 80 cases.
area=net
. "$(dirname "$0")/lib.sh"
need "$gold/net/laguna_a_net_watch.netgold" "the multiplayer goldens (tools/net_gold/README.md)"
NET_GOLD_OUT=$out/check sh tools/net_gold/check.sh > "$out/check.txt" 2>&1
# "<golden> <way> | steps ... | IDENTICAL ..."
grep -E '^[a-z_]+ (desktop|browser|gateway|wasm) \|' "$out/check/check.log" | while IFS='|' read -r name _ v; do
    case "$v" in *IDENTICAL*) verdict "$name" IDENTICAL ;; *) verdict "$name" DIFFERENT "$v" ;; esac
done
grep -q 'numbers differ' "$out/check/check.log" && verdict "the four ways' numbers" DIFFERENT
summary
