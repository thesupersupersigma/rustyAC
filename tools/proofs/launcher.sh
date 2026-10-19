#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The launcher (Tasks 35-37b): every request list of re/gold/launcher (made by launcher_record.sh
# from the click scripts of tools/proofs/launcher/) answered by the game's own handlers
# (tools/launcher_oracle, AssettoCorsa.exe's code in .NET) and by the port (rustyac-launcher
# --transcript), each from its own empty player folder, answer for answer. One case per list.
area=launcher
. "$(dirname "$0")/lib.sh"
ls "$gold"/launcher/*.req > /dev/null 2>&1 || need "$gold/launcher/practice.req" "the request lists (sh tools/proofs/launcher_record.sh)"
dotnet build --locked-mode -c Release tools/launcher_oracle/launcher_oracle.csproj > "$out/dotnet.txt" 2>&1 || { echo "dotnet build failed" >&2; exit 2; }
cargo build --release --locked --quiet -p rustyac-launcher || exit 2
LO=$(ls tools/launcher_oracle/bin/Release/net*/launcher_oracle.exe | head -n 1)
# the oracle takes only a fake player folder under re\scratch (never Documents); it is deleted at the end
fake=$repo/re/scratch/proofs/launcher
for req in "$gold"/launcher/*.req; do
    name=$(basename "$req" .req)
    rm -rf "$fake"; mkdir -p "$fake/o" "$fake/p"
    "$LO" --docs "$(cygpath -w "$fake/o")" --requests "$(cygpath -w "$req")" --out "$(cygpath -w "$out/$name.oracle.txt")" > "$out/$name.oracle.log" 2>&1
    target/release/rustyac-launcher.exe --transcript "$req" "$out/$name.port.txt" --docs "$fake/p" --ac-root "$ac" > "$out/$name.port.log" 2>&1
    python tools/proofs/tdiff.py "$out/$name.oracle.txt" "$out/$name.port.txt" > "$out/$name.diff.txt" 2>&1
    # the player folders the two sides leave must be the same too
    diff -r "$fake/o" "$fake/p" >> "$out/$name.diff.txt" 2>&1; same_dir=$?
    res=$(tail -n 1 "$out/$name.diff.txt")
    if [ "$same_dir" = 0 ] && ! grep -q '^DIFFERENT' "$out/$name.diff.txt" && grep -q 'unchanged' "$out/$name.oracle.log"; then
        verdict "launcher $name" IDENTICAL "$(grep -m1 ' the same ' "$out/$name.diff.txt")"
    else
        verdict "launcher $name" DIFFERENT "$(grep -m1 -E '^DIFFERENT|^Only|differ' "$out/$name.diff.txt") $res"
    fi
done
rm -rf "$fake"
summary
