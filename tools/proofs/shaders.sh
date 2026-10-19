#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The DXBC to WGSL translator (Tasks 38, 38b): every one of the game's shaders reflected and
# translated, then each pair drawn by D3D11 WARP and by wgpu (SwiftShader's Vulkan, the DLL Chrome
# ships, in re/gold/shaders; and D3D12 WARP). Each table must be Task 38b's again, row for row.
area=shaders
. "$(dirname "$0")/lib.sh"
need "$gold/shaders/swiftshader/vk_swiftshader_icd.json" "SwiftShader (re/gold/shaders/README.md)"
cargo test --release --locked --quiet -p rustyac-render --test dxbc_reflect > "$out/reflect.txt" 2>&1 \
    && verdict "dxbc_reflect" IDENTICAL || verdict "dxbc_reflect" DIFFERENT "$(grep -m1 -E 'FAILED|panicked' "$out/reflect.txt")"
cargo run --release --locked --quiet -p rustyac-dxbc --example shader_table -- "$ac/system/shaders/win" > "$out/table.txt" 2>&1
grep -qE '72 of 72' "$out/table.txt" && verdict "shader_table" IDENTICAL "$(grep -m1 -oE '72 of 72[^|]*' "$out/table.txt")" \
    || verdict "shader_table" DIFFERENT "$(tail -n 1 "$out/table.txt")"
export VK_DRIVER_FILES="$(cygpath -w "$gold/shaders/swiftshader/vk_swiftshader_icd.json")"
for run in "swift_flat --adapter swiftshader --flat" "warp_flat --adapter warp --flat"; do
    set -- $run; name=$1; shift
    cargo run --release --locked --quiet -p rustyac-render --features wgpu --example shader_proof -- "$@" > "$out/$name.md" 2> "$out/$name.err"
    # the table's rows (a header line names the device and may differ by driver build)
    if diff <(grep '^| ' "$gold/shaders/$name.md") <(grep '^| ' "$out/$name.md") > "$out/$name.diff"; then
        verdict "shader_proof $name" IDENTICAL "$(tail -n 1 "$out/$name.md")"
    else
        verdict "shader_proof $name" DIFFERENT "$(grep -c '^[<>]' "$out/$name.diff") rows changed"
    fi
done
summary
