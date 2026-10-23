#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The renderer on kgl's wgpu backend (Task 38h), not an area of run_all.sh: every row of
# tools/proofs/render_list.txt drawn by the game on WARP and by the port on Direct3D 11
# (render_oracle compare), then by the port on wgpu with each adapter of ADAPTERS. Per case: the
# kgl call logs of the two port backends line by line, and the wgpu frames against the game's
# pixels (PSNR, largest channel difference, pixels off by more than 8; pictures game | wgpu | x8).
#   ONLY=<regex>       only the matching case names
#   ADAPTERS="gpu swiftshader"  (default "gpu"; swiftshader needs SWIFTSHADER=<vk_swiftshader_icd.json>,
#                      e.g. the one beside Chrome's vk_swiftshader.dll)
#   SEQ_ADAPTERS       the adapters for sequences (default "gpu": SwiftShader takes minutes on one)
#   SEQ_EVERY=k        only every k-th sequence; sequences compare the middle and the last frame
# Output: target/proofs/render_wgpu/ (OUT), the table: python tools/proofs/render_wgpu.py table <out>
set -u
cd "$(dirname "$0")/../.."
S=tools/proofs
OUT=${OUT:-target/proofs/render_wgpu}
D3D=tools/render_oracle/target/release/render_oracle.exe
WG=target/ro_wgpu/release/render_oracle.exe
mkdir -p "$OUT"
cargo build --release --locked --quiet --manifest-path tools/render_oracle/Cargo.toml || exit 2
(cd tools/render_oracle && CARGO_TARGET_DIR=../../target/ro_wgpu cargo build --release --locked --quiet --features wgpu) || exit 2
[ -n "${SWIFTSHADER:-}" ] && export VK_DRIVER_FILES="$(cygpath -w "$SWIFTSHADER")"
grep -vE '^\s*(#|$)' tools/proofs/render_list.txt | grep -E "^(${ONLY:-.*}) " | while IFS='|' read -r name args; do
    name=$(echo "$name" | tr -d ' ')
    [ -f "$OUT/$name.done" ] && continue
    for p in $(echo "$args" | grep -oE 'POSE:[^ ]+'); do
        spec=${p#POSE:}; tape=${spec%:*}; frame=${spec##*:}
        mkdir -p "$OUT/poses"
        "$D3D" pose-of --tape "$tape" --tape-from "$frame" --pose "$OUT/poses/$name.pose" > /dev/null 2>&1 < /dev/null
        args=$(echo "$args" | sed "s#$p#$OUT/poses/$name.pose#")
    done
    dump=""
    n=$(echo "$args" | grep -oE -- '--frames [0-9]+' | grep -oE '[0-9]+')
    adapters=${ADAPTERS:-gpu}
    # SwiftShader takes minutes per sequence: sequences on the GPU only unless SEQ_ADAPTERS says
    [ -n "$n" ] && { dump="--dump 0,$((n / 2)),$((n - 1))"; adapters=${SEQ_ADAPTERS:-gpu}; }
    # SEQ_EVERY=k: only every k-th sequence of the list (a sequence takes about two minutes)
    if [ -n "$n" ]; then
        seq_i=$(( ${seq_i:--1} + 1 ))
        [ $((seq_i % ${SEQ_EVERY:-1})) -ne 0 ] && continue
    fi
    t0=$(date +%s)
    rm -rf "$OUT/w"; mkdir -p "$OUT/w/d3d"
    "$D3D" compare $args $dump --label "$name" --root "$OUT/root" --out "$OUT/w/d3d" > "$OUT/$name.d3d.txt" 2>&1 < /dev/null
    t1=$(date +%s)
    for a in $adapters; do
        mkdir -p "$OUT/w/$a"
        RENDER_ORACLE_ADAPTER=$a "$WG" run --side port $args $dump --label "$name" --root "$OUT/root" --out "$OUT/w/$a" > "$OUT/$name.$a.txt" 2>&1 < /dev/null
        t2=$(date +%s)
        python "$S/render_wgpu.py" frame "$OUT/w" "$name" "$a" > "$OUT/$name.$a.res" 2>&1
        echo "seconds d3d=$((t1 - t0)) $a=$((t2 - t1))" >> "$OUT/$name.$a.res"
        for j in "$OUT"/w/"$name"*.side.jpg; do [ -f "$j" ] && mv "$j" "$OUT/$(basename "$j" .side.jpg).$a.jpg"; done
        t1=$(date +%s)
    done
    echo "$name d3d: $(grep -E '^(command log|pixels):' "$OUT/$name.d3d.txt" | cut -c1-40 | tr '\n' ' ')"
    for a in $adapters; do echo "  $a: $(grep -E '^(kgl log|frames)' "$OUT/$name.$a.res" | tr '\n' ' ')"; done
    rm -rf "$OUT/w"
    touch "$OUT/$name.done"
done
