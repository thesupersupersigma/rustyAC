#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Sends to the proof farm (docs/proof_farm.md) what its areas read, over SMB with robocopy, only
# what changed (a file of the same size and time is skipped; /XO never puts an older file over a
# newer one). The farm's copy: <share>\repo (as this repo, for RUSTYAC_REPO) and <share>\ac (the AC
# files the areas read, for AC_ROOT). Nothing is ever deleted there except under repo\tools\proofs
# and repo\target\prebuilt, and every source is named below, so nothing under a served folder
# (web/, target/web*) goes.
#   FARM_SHARE=//<vm>/rustyac-farm bash tools/proofs/farm_sync.sh [part ...]
# parts (default: programs physics ac repo): programs (target/prebuilt's manifest, the proof scripts,
# cardata/), physics (what the physics area reads), ac (the rest of the install, ~64 GB), repo (oracle/,
# re/gold). Each part prints "sync <part>: <bytes> in <s> s (<MB/s>)"; robocopy's logs are in
# target/farm_sync/.
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd); cd "$repo" || exit 2
# the share as //<vm>/rustyac-farm or \\<vm>\rustyac-farm; anything else (a lost backslash makes
# \<vm>\... a folder on this PC's C:) stops here, and so does a share that does not answer
share_u=$(echo "$FARM_SHARE" | tr '\\' /)
case $share_u in //?*/?*) ;; *) echo "FARM_SHARE='//<vm>/rustyac-farm' is not set (got '$FARM_SHARE')" >&2; exit 2 ;; esac
[ -d "$share_u/repo" ] || { echo "the farm share $share_u does not answer (no repo folder)" >&2; exit 2; }
FARM_SHARE=$(echo "$share_u" | tr / '\\')
ac=${AC_ROOT:-"C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"}
logs=$repo/target/farm_sync; mkdir -p "$logs"
export MSYS2_ARG_CONV_EXCL='*'  # robocopy's /E and the \\share paths stay as written
bytes=0

# rc <src dir> <dst under the share> [robocopy options and file names]: one robocopy, its bytes counted
rc() {
    src=$(cygpath -w "$1"); dst="$FARM_SHARE\\$(echo "$2" | tr / '\\')"; shift 2; t0=$(date +%s)
    robocopy "$src" "$dst" "$@" /XO /MT:8 /J /R:2 /W:5 /NP /NDL /NFL /BYTES > "$logs/last.txt" 2>&1
    code=$?
    cat "$logs/last.txt" >> "$logs/$part.txt"
    [ $code -lt 8 ] || { echo "robocopy $src -> $dst failed ($code, $logs/$part.txt)" >&2; exit 2; }
    # the summary's "Bytes :  total  copied ..." line
    b=$(awk '/^ *Bytes :/ { print $4; exit }' "$logs/last.txt"); bytes=$((bytes + ${b:-0}))
    echo "  $src: ${b:-0} bytes in $(( $(date +%s) - t0 )) s"
}

dlls="msvcr120.dll vcruntime140.dll d3dcompiler_43.dll d3dx11_43.dll xinput1_3.dll"
part_programs() {
    [ -f target/prebuilt/manifest.txt ] || { echo "no target/prebuilt/manifest.txt (bash tools/proofs/prebuild.sh)" >&2; exit 2; }
    rc tools/proofs repo/tools/proofs /MIR
    rc tools/net_gold repo/tools/net_gold /E
    rc target/prebuilt repo/target/prebuilt /MIR /XF $dlls  # the DLLs below stay
    # every file of the manifest, folder by folder; the PC's MSVCR120.dll and VCRUNTIME140.dll beside
    # each one (the exe's folder comes first in the DLL search: the processor stays the only
    # difference from the PC, F1), and the DirectX June 2010 DLLs acs.exe imports, which a plain
    # Windows lacks (F2 part 3)
    while read -r d f; do
        # shellcheck disable=SC2086
        rc "$d" "repo/$d" $f
        rc /c/Windows/System32 "repo/$d" $dlls
    done < <(awk '{ p = $2; sub(/^[*]/, "", p); n = split(p, s, "/"); d = substr(p, 1, length(p) - length(s[n]) - 1)
        fs[d] = fs[d] " " s[n] } END { for (d in fs) print d fs[d] }' target/prebuilt/manifest.txt)
    rc cardata repo/cardata /E
    rc "$ac/system" ac/system /E
    rc "$ac" ac acs.exe
}
part_physics() {
    # the one-car folders, every .mcrec folder (its top level), the tracks the recordings name, the
    # cars' colliders
    for d in oracle/car*/; do rc "$d" "repo/${d%/}" /E; done
    while read -r d; do rc "$d" "repo/$d" /LEV:1; done < <(find oracle/ -name '*.mcrec' | sed 's|/[^/]*$||' | sort -u)
    while read -r t; do rc "$ac/content/tracks/$t" "ac/content/tracks/$t" /E; done < <(find oracle/ -name '*.mcrec' \
        -exec sh -c 'head -c 4096 "$1" | tr -c "[:print:]\n" "\n" | grep -m1 -oE "track=[^ ]*"' _ {} \; | sed 's|.*[=/\\]||' | sort -u)
    rc "$ac/content/cars" ac/content/cars collider.kn5 /S
    # the old AI lines some recordings drove (their header's ai_folder)
    rc re/gold/roots repo/re/gold/roots /E
}
# the rest, for the other areas (F2 part 3): the whole install but sdk/ (Kunos' editors, never read),
# then every kept result and recording but re/gold/web (the browser's, which stays on the PC), and the
# two repo files areas read besides the programs (python's test apps, csp's race.ini)
part_ac() {
    rc "$ac" ac /E /XD "$(cygpath -w "$ac/sdk")"
}
part_repo() {
    rc oracle repo/oracle /E
    rc re/gold repo/re/gold /E /XD "$(cygpath -w re/gold/web)"
    rc tools/render_oracle/py_apps repo/tools/render_oracle/py_apps /E
    rc tools/csp_oracle/src repo/tools/csp_oracle/src main.rs
}

for part in ${*:-programs physics ac repo}; do
    [ "$(type -t "part_$part")" = function ] || { echo "no part $part" >&2; exit 2; }
    : > "$logs/$part.txt"; bytes=0; s=$(date +%s)
    "part_$part"
    t=$(( $(date +%s) - s ))
    echo "sync $part: $bytes bytes in $t s ($(awk -v b=$bytes -v t=$t 'BEGIN { printf "%.1f", t ? b / t / 1048576 : 0 }') MB/s)"
done
