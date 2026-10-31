#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Every proof of the project (see tools/proofs/README.md):
#   bash tools/proofs/run_all.sh [--full]          every area, every case (≈ the milestone run), the port
#                                                  against the game's kept results (--rerecord: the game again too)
#   bash tools/proofs/run_all.sh --quick           every area on the small samples of tools/proofs/quick/ (≈ 30 min)
#   bash tools/proofs/run_all.sh --changed <tag>   the areas the files changed since <tag> ask for
#                                                  (tools/proofs/areas.map), every case of each
#   ... [--rerecord] [--dry-run] [area ...]        record the game's recordings again / only say which
#                                                  areas would run / only these areas
# Needs: this repository, the ignored oracle/ and re/gold/ folders, the AC install (read only).
# Each area writes into target/proofs/<area> and ends with "PROOF <area> cases= identical= different= skipped=";
# the table below is made from those lines. Exit code 0 only when no area has a difference.
# The areas run in lanes (below); the shared programs are built first with 8 jobs, then what an
# area builds for itself with 2 jobs a lane (CARGO_BUILD_JOBS), 8 in all.
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
all="physics fresh sound render gui python replays net browser ai_page shaders launcher dumps"
# the lanes, each one area after another, all lanes at once: two lanes load the game's code (the
# render oracle's and the car oracle's), never two areas of one lane; the others never load it
# (a render or GUI compare keeps about 1.5 of the 16 threads busy, Task P1)
lanes="render gui python|fresh sound replays launcher|physics dumps shaders|net browser ai_page"
mode=full; tag=; want=; dry=
while [ $# -gt 0 ]; do
    case "$1" in
        --quick) mode=quick ;;
        --full) mode=full ;;
        --changed) mode=changed; tag=$2; shift; [ -n "$tag" ] || { echo "--changed needs a tag" >&2; exit 2; } ;;
        --rerecord) export PROOF_RERECORD=1 ;;
        --dry-run) dry=1 ;;
        -*) echo "unknown option $1" >&2; exit 2 ;;
        *) want="$want $1" ;;
    esac
    shift
done
for a in $want; do [ -f "$here/$a.sh" ] || { echo "no area $a (areas: $all)" >&2; exit 2; }; done
areas=${want:-$all}
out=${PROOF_OUT:-$repo/target/proofs}
mkdir -p "$out/rows"
case $mode in
    quick) export PROOF_QUICK=1 ;;
    full) ;;
    changed)
        git -C "$repo" rev-parse -q --verify "$tag^{commit}" > /dev/null || { echo "no tag $tag" >&2; exit 2; }
        echo "files changed since $tag:"
        # committed since <tag> and not committed yet
        picked=$(git -C "$repo" diff --name-only "$tag" | PROOF_SINCE=$tag bash "$here/areas.sh" 2> "$out/changed.txt")
        sed 's/^/  /' "$out/changed.txt"
        areas=$(for a in $areas; do case " $picked " in *" $a "*) printf '%s ' "$a" ;; esac; done)
        ;;
esac
areas=$(echo $areas)
echo "mode $mode: ${areas:-no area}"
[ -n "$areas" ] && [ -z "$dry" ] || exit 0

rm -f "$out"/rows/*.row
# each lane's areas of this run, in the lane's order
picked_lanes=$(echo "$lanes" | tr '|' '\n' | while read -r lane; do
    l=$(for a in $lane; do case " $areas " in *" $a "*) printf '%s ' "$a" ;; esac; done)
    [ -n "$l" ] && echo "$l"
done)
jobs=${CARGO_BUILD_JOBS:-8}
export CARGO_BUILD_JOBS=$jobs
start_all=$(date +%s)

# every program more than one area runs, built once here before any area starts (the areas skip
# what PROOF_BUILT names): no area relinks an exe another lane is running, and a source changed
# during the run changes nothing in it
cd "$repo" || exit 2
built=
for t in car_oracle chassis_compare render_oracle audio_oracle; do
    case $t in
        car_oracle) uses="physics fresh replays" ;; chassis_compare) uses="physics fresh" ;;
        render_oracle) uses="render gui python fresh" ;; audio_oracle) uses="sound" ;;
    esac
    for u in $uses; do
        case " $areas " in *" $u "*)
            echo "building $t"
            cargo build --release --locked --quiet --manifest-path "tools/$t/Cargo.toml" && built="$built $t"
            break ;;
        esac
    done
done
case " $areas " in *" fresh "*|*" sound "*|*" replays "*|*" browser "*|*" ai_page "*|*" dumps "*)
    echo "building rustyac, replay_hash"
    cargo build --release --locked --quiet -p rustyac-game --bin rustyac --bin replay_hash && built="$built rustyac replay_hash" ;;
esac
case " $areas " in *" fresh "*)
    echo "building track_survey"
    cargo build --release --locked --quiet -p rustyac-physics --bin track_survey && built="$built track_survey" ;;
esac
case " $areas " in *" launcher "*)
    echo "building rustyac-launcher"
    cargo build --release --locked --quiet -p rustyac-launcher && built="$built rustyac-launcher" ;;
esac
export PROOF_BUILT=$built
n=$(echo "$picked_lanes" | grep -c .)
export CARGO_BUILD_JOBS=$(( jobs / n > 0 ? jobs / n : 1 ))

run_area() {
    a=$1
    start=$(date +%s)
    bash "$here/$a.sh" > "$out/$a.log" 2>&1
    code=$?
    secs=$(( $(date +%s) - start ))
    line=$(grep '^PROOF ' "$out/$a.log" | tail -n 1)
    if [ -n "$line" ]; then
        c=$(echo "$line" | sed -E 's/.*cases=([0-9]+).*/\1/'); i=$(echo "$line" | sed -E 's/.*identical=([0-9]+).*/\1/'); d=$(echo "$line" | sed -E 's/.*different=([0-9]+).*/\1/')
        k=$(echo "$line" | sed -nE 's/.*skipped=([0-9]+).*/\1/p')
    else
        c=-; i=-; d="did not run ($(tail -n 1 "$out/$a.log"))"; k=-
    fi
    printf '| %s | %s | %s | %s | %s | %dm%02ds |\n' "$a" "$c" "$i" "$d" "${k:-0}" $((secs / 60)) $((secs % 60)) | tee "$out/rows/$a.row"
    [ "$code" = 0 ] || : > "$out/rows/$a.bad"
}
rm -f "$out"/rows/*.bad
while read -r lane; do
    ( for a in $lane; do run_area "$a"; done ) &
done <<LANES
$picked_lanes
LANES
wait

secs=$(( $(date +%s) - start_all ))
table=$out/table.md
{
    echo "| area | cases | identical | different | skipped | time |"
    echo "|---|---|---|---|---|---|"
    for a in $areas; do cat "$out/rows/$a.row"; done
    printf '| all (%s, wall clock) | | | | | %dh%02dm |\n' "$mode" $((secs / 3600)) $((secs % 3600 / 60))
} > "$table"
echo; cat "$table"
# every case left out, with its reason (red when there is one: nothing drops out silently)
for a in $areas; do grep -h '^SKIPPED ' "$out/$a/cases.log" 2> /dev/null; done > "$out/skipped.txt"
n=$(grep -c '' "$out/skipped.txt")
if [ "$n" -gt 0 ]; then printf '\n\033[31m%s cases skipped\033[0m (%s):\n' "$n" "$out/skipped.txt"; sed 's/^SKIPPED /  /' "$out/skipped.txt"
else printf '\n0 cases skipped\n'; fi
! ls "$out"/rows/*.bad > /dev/null 2>&1
