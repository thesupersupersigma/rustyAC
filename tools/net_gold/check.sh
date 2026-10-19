#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the port against the 20 multiplayer goldens of gold.sh in four ways: the desktop
# transport, the browser's WebSocket transport, through the gateway, and the wasm build
# under wasmtime. Every way must say IDENTICAL and print the same numbers and digests.
# Needs the AC folder, cardata/ and wasmtime (docs/port/multiplayer.md, "Golden tapes").
#
#   sh tools/net_gold/check.sh [golden name ...]    (default: all 20; $NET_GOLD_WAYS: which ways)
#   -> one line per golden and way in $NET_GOLD_OUT/check.log (target/proofs/net_check)
set -eu
# in Windows form (C:/...): rustc and wasmtime get it in places no path conversion reaches
repo=$(cd "$(dirname "$0")/../.." && { pwd -W 2>/dev/null || pwd; })
gold=$repo/re/gold/net
out=${NET_GOLD_OUT:-$repo/target/proofs/net_check}
ac=${AC_ROOT:-"C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"}
wasmtime=$repo/target/web-tools/wasmtime/bin/wasmtime.exe
mkdir -p "$out"
cd "$repo"

cargo build --release --locked -p rustyac-net --example net_golden
CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="--sysroot $repo/target/wasm-sysroot" \
    cargo build --release --locked -p rustyac-net --example net_golden --target wasm32-wasip1
exe=$repo/target/release/examples/net_golden.exe
wasm=$repo/target/wasm32-wasip1/release/examples/net_golden.wasm

names=$*
[ -n "$names" ] || names=$(cd "$gold" && ls *.netgold | sed 's/\.netgold$//')
: > "$out/check.log"
failed=0
for name in $names; do
    first=
    for way in ${NET_GOLD_WAYS:-desktop browser gateway wasm}; do
        case $way in
            desktop) "$exe" "$gold/$name.netgold" --root "$ac" > "$out/$name.$way" 2>&1 || true ;;
            browser|gateway) "$exe" "$gold/$name.netgold" --root "$ac" --$way > "$out/$name.$way" 2>&1 || true ;;
            # the browser's client built for wasm; the repo and the AC folder are the only folders it sees
            wasm) MSYS_NO_PATHCONV=1 "$wasmtime" run --dir "$repo::/repo" --dir "$ac::/ac" "$wasm" \
                "/repo/re/gold/net/$name.netgold" --cardata /repo/cardata --root /ac --browser > "$out/$name.$way" 2>&1 || true ;;
        esac
        numbers=$(grep '^steps ' "$out/$name.$way" || echo "no numbers")
        verdict=$(grep -E '^(IDENTICAL|DIFFERENT)' "$out/$name.$way" || tail -1 "$out/$name.$way")
        echo "$name $way | $numbers | $verdict" | tee -a "$out/check.log"
        case $verdict in IDENTICAL*) ;; *) failed=$((failed + 1)) ;; esac
        [ -n "$first" ] || first=$numbers
        [ "$numbers" = "$first" ] || { echo "$name: the $way way's numbers differ from the first way's" | tee -a "$out/check.log"; failed=$((failed + 1)); }
    done
done
echo "failed: $failed" | tee -a "$out/check.log"
[ $failed = 0 ]
