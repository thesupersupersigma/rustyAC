#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Multiplayer (Tasks 25-26, 37, NH-2): the 24 goldens of re/gold/net checked four ways by
# tools/net_gold/check.sh (desktop, browser, gateway, wasm): 96 cases.
area=net
. "$(dirname "$0")/lib.sh"
# the goldens were made by the game on MSVCR120's FMA3 bodies (the PC): off FMA3 the native ways take
# their exact Rust copies (pure.rs, as physics does; docs/proof_farm.md, F2 part 3)
[ "$maths_path" = FMA3 ] || export RUSTYAC_MATH=pure
need "$gold/net/laguna_a_net_watch.netgold" "the multiplayer goldens (tools/net_gold/README.md)"
# quick: only the goldens named in tools/proofs/quick/net.txt (each still four ways)
# shellcheck disable=SC2046
NET_GOLD_OUT=$out/check sh tools/net_gold/check.sh $([ -n "$PROOF_QUICK" ] && cat "$quick_re") > "$out/check.txt" 2>&1
# "<golden> <way> | steps ... | IDENTICAL ..."
grep -E '^[a-z_]+ (desktop|browser|gateway|wasm) \|' "$out/check/check.log" | while IFS='|' read -r name _ v; do
    case "$v" in *IDENTICAL*) verdict "$name" IDENTICAL ;; *) verdict "$name" DIFFERENT "$v" ;; esac
done
grep -q 'numbers differ' "$out/check/check.log" && verdict "the four ways' numbers" DIFFERENT
summary
