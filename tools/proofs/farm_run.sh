#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The proof farm's half of `run_all.sh --farm` (docs/proof_farm.md), run on the VM by farm_task.cmd
# as a scheduled task in the logged-in user's session (session 0 has no desktop: no swap chains for
# WARP, no screen modes for the launcher). The farm folder is the one above this repo copy:
#   <farm>/repo  this copy (RUSTYAC_REPO), the prebuilt programs of prebuild.sh (RUSTYAC_PREBUILT)
#   <farm>/ac    the AC files the areas read (AC_ROOT)
#   <farm>/home  the run's own home (and so its own proof lock)
#   <farm>/run   args (run_all.sh's arguments, one a line), pull (how many lanes take from the queue,
#                PROOF_PULL) and queue/ (the queue shared with the PC, PROOF_QUEUE), all from the PC;
#                started (written first) and exit (run_all.sh's exit code, written last)
repo=$(cd "$(dirname "$0")/../.." && pwd)
farm=$(dirname "$repo")
run=$farm/run
export HOME=$farm/home
USERPROFILE=$(cygpath -w "$HOME")
RUSTYAC_REPO=$(cygpath -w "$repo")
AC_ROOT=$(cygpath -m "$farm/ac")
export USERPROFILE RUSTYAC_REPO AC_ROOT RUSTYAC_PREBUILT=1
# wasmtime's default cache config needs the home's AppData\Roaming
mkdir -p "$HOME/AppData/Roaming"
# AC's python33.dll is loaded with LOAD_WITH_ALTERED_SEARCH_PATH (no exe folder in the search): its
# MSVCR100.dll (farm_sync.sh puts it beside each exe) is found through PATH
export PATH="$repo/tools/render_oracle/target/release:$PATH"
PROOF_PULL=$(tr -d '\r' < "$run/pull")
export PROOF_PULL PROOF_QUEUE=$run/queue
mapfile -t args < <(tr -d '\r' < "$run/args")
cd "$repo" || exit 2
date +%s > "$run/started"
bash tools/proofs/run_all.sh "${args[@]}" > "$run/out.txt" 2>&1
code=$?
# the kept results this run made, for farm.sh to copy back to the PC
find re/gold/fresh/rec/ re/gold/dumps/ -type f -newer "$run/started" > "$run/new_kept.txt"
echo $code > "$run/exit"
