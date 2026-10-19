#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The launcher (Tasks 35-37b): every request list of re/gold/launcher (made by launcher_record.sh
# from the click scripts of tools/proofs/launcher/) answered by the game's own handlers
# (tools/launcher_oracle, AssettoCorsa.exe's code in .NET) and by the port (rustyac-launcher
# --transcript), each from its own empty player folder, answer for answer. One case per list.
area=launcher
. "$(dirname "$0")/lib.sh"
ls ${LAUNCHER_LISTS:-$gold/launcher}/*.req > /dev/null 2>&1 || need "$gold/launcher/practice.req" "the request lists (sh tools/proofs/launcher_record.sh)"
# NuGet's and dotnet's own folders inside the repo (never the user's profile or the system temp)
nu=$repo/target/nuget; mkdir -p "$nu/tmp"
export NUGET_PACKAGES="$(cygpath -w "$nu/packages")" DOTNET_CLI_HOME="$(cygpath -w "$nu/home")" TEMP="$(cygpath -w "$nu/tmp")" TMP="$(cygpath -w "$nu/tmp")" DOTNET_CLI_TELEMETRY_OPTOUT=1
{ dotnet restore --locked-mode --source https://api.nuget.org/v3/index.json tools/launcher_oracle/launcher_oracle.csproj &&
  dotnet build -c Release --no-restore tools/launcher_oracle/launcher_oracle.csproj; } > "$out/dotnet.txt" 2>&1 || { echo "dotnet build failed (see $out/dotnet.txt)" >&2; exit 2; }
cargo build --release --locked --quiet -p rustyac-launcher || exit 2
LO=$(ls tools/launcher_oracle/bin/Release/launcher_oracle.exe tools/launcher_oracle/bin/Release/net*/launcher_oracle.exe 2> /dev/null | head -n 1)
# the oracle takes only a fake player folder under re\scratch (never Documents); it is deleted at the end
fake=$repo/re/scratch/proofs/launcher
for req in ${LAUNCHER_LISTS:-$gold/launcher}/*.req; do
    name=$(basename "$req" .req)
    # both sides in the same folder (answers name it), one after the other, the folders kept apart after
    rm -rf "$fake"; mkdir -p "$fake/docs"
    "$LO" --docs "$(cygpath -w "$fake/docs")" --requests "$(cygpath -w "$req")" --out "$(cygpath -w "$out/$name.oracle.txt")" > "$out/$name.oracle.log" 2>&1
    mv "$fake/docs" "$fake/o"; mkdir -p "$fake/docs"
    target/release/rustyac-launcher.exe --transcript "$req" "$out/$name.port.txt" --docs "$fake/docs" --ac-root "$ac" > "$out/$name.port.log" 2>&1
    mv "$fake/docs" "$fake/p"
    python tools/proofs/tdiff.py "$out/$name.oracle.txt" "$out/$name.port.txt" > "$out/$name.diff.txt" 2>&1
    # the player folders the two sides leave must be the same too (backups/ is named by the time)
    diff -r -x backups "$fake/o" "$fake/p" >> "$out/$name.diff.txt" 2>&1; same_dir=$?
    res=$(tail -n 1 "$out/$name.diff.txt")
    if [ "$same_dir" = 0 ] && ! grep -q '^DIFFERENT' "$out/$name.diff.txt" && grep -q 'unchanged' "$out/$name.oracle.log"; then
        verdict "launcher $name" IDENTICAL "$(grep -m1 ' the same ' "$out/$name.diff.txt")"
    else
        verdict "launcher $name" DIFFERENT "$(grep -m1 -E '^DIFFERENT|^Only|differ' "$out/$name.diff.txt") $res"
    fi
done
rm -rf "$fake"
summary
