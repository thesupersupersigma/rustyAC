# SPDX-License-Identifier: GPL-3.0-or-later
#
# Sourced by run_all.sh and lib.sh (needs $repo). RUSTYAC_PREBUILT=1: run the programs
# tools/proofs/prebuild.sh built, with no cargo, dotnet or git: this folder's stand-ins go first on
# PATH, and every file of the manifest is checked once a run (a changed one stops it).
[ -n "$RUSTYAC_PREBUILT" ] || return 0
case ":$PATH:" in *":$repo/tools/proofs/prebuilt:"*) ;; *) export PATH="$repo/tools/proofs/prebuilt:$PATH" ;; esac
if [ -z "$PREBUILT_CHECKED" ]; then
    (cd "$repo" && sha256sum --quiet -c target/prebuilt/manifest.txt) ||
        { echo "the prebuilt programs are not the manifest's (bash tools/proofs/prebuild.sh)" >&2; exit 2; }
    export PREBUILT_CHECKED=1
fi
# what the areas would otherwise ask git for
export DUMPS_OLD=${DUMPS_OLD:-$repo/target/proofs/old_exe/$(cat "$repo/target/prebuilt/dumps_tag")}
